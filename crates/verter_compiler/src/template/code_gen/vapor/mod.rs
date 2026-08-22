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
mod repeated_reads;
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
use crate::template::oxc::types::{Dynamism, OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use oxc_ast::ast::{
    ArrayExpressionElement, AssignmentTarget, BindingPattern, Expression, ObjectPropertyKind,
    PropertyKey,
};
use rustc_hash::FxHashSet;

use super::binding::BindingResolver;
use super::shared::helpers::{self, VaporHelper};
use super::types::{
    CodeGenOutput, MergedConstructKind, SegmentedOverwriteAuthority, VaporCounters, VaporEffect,
    VaporElementState, VaporRootElement,
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

/// Whether `expr` (trimmed) is a compile-time-constant JS literal —
/// string/number/boolean/`null`/`undefined` — official's narrow leaf case
/// of `isDirectConstantValue`/`isDirectConstantAst` (rc.5
/// `generators/props.ts`). Deliberately narrower than official's full
/// recursive array/object/template-literal-of-constants coverage (no
/// fixture needs it); anything not recognized here conservatively wraps in
/// a getter at the call site, matching official's own non-constant
/// fallback — never emits an unwrapped non-constant.
fn is_direct_constant_expr(expr: &str) -> bool {
    matches!(expr, "true" | "false" | "null" | "undefined")
        || is_string_literal_text(expr)
        || is_numeric_literal_text(expr)
}

fn is_string_literal_text(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
        && !bytes[1..bytes.len() - 1].contains(&bytes[0])
}

fn is_numeric_literal_text(expr: &str) -> bool {
    !expr.is_empty()
        && expr.bytes().any(|b| b.is_ascii_digit())
        && expr
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_digit() || b == b'.' || (i == 0 && b == b'-'))
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

/// Detect `<component :is="expr">` / `<component v-bind:is="expr">` and
/// resolve its bound expression. Returns `Some((resolved_expr, prop_idx))`
/// — mirrors `vdom::component::resolve_dynamic_component`, the equivalent
/// VDOM-backend detector; Vapor has no such helper of its own until now.
fn resolve_vapor_dynamic_component<'a>(
    el: &ElementNode,
    tag_name: &str,
    source: &str,
    oxc_el: Option<&OxcParsedElement<'a>>,
    resolver: &BindingResolver<'a>,
    force_js: bool,
) -> Option<(String, usize)> {
    if tag_name != "component" {
        return None;
    }
    for (i, prop) in el.props.iter().enumerate() {
        if !prop.is_directive {
            continue;
        }
        let directive_name = &source[prop.start as usize..prop.name_end as usize];
        let is_bind = directive_name == ":" || directive_name == "v-bind";
        if !is_bind {
            continue;
        }
        let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) else {
            continue;
        };
        let arg_name = &source[as_ as usize..ae as usize];
        if arg_name != "is" {
            continue;
        }
        let resolved_expr = if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value = &source[vs as usize..ve as usize];
            let oxc_exp = find_prop_oxc_exp(oxc_el, i);
            resolve_expr(value, vs, oxc_exp, resolver, force_js)
        } else {
            // Value-less `:is` — Vue 3.4 same-name shorthand: `:is` == `:is="is"`.
            resolve_expr(arg_name, as_, None, resolver, force_js)
        };
        return Some((resolved_expr, i));
    }
    None
}

/// Official `setInsertionState(parent, anchor)` 2nd arg (vendored rc.5):
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
/// rc.5). `None` when official omits it (`flags === 1`: bare v-if, positive
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

/// Official `_createFor` trailing flags (`genForFlags`, vendored rc.5).
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

/// A destructured leaf's accessor, relative to the shared wrapper base
/// (`_for_item{depth}.value` / `_slotProps{depth}`) `render` is given.
enum DestructureAccessor {
    /// Plain member-path suffix appended directly to the base:
    /// `{base}{suffix}`.
    Path(String),
    /// A rest element (`{ id, ...rest }`) — official's
    /// `_getRestElement({base}{suffix}, [excludedKeys...])`, excluding the
    /// STATIC sibling keys already destructured at the SAME nesting level
    /// (rc.5 `packages/compiler-vapor/src/transforms/vFor.ts`, confirmed
    /// directly against the vendored dist and the real with-vapor runtime).
    Rest {
        suffix: String,
        excluded_keys: Vec<String>,
    },
    /// A default value (`{ id = 99 }`) — official's
    /// `_getDefaultValue({base}{suffix}, () => (resolvedDefaultExpr))`.
    Default {
        suffix: String,
        resolved_default_expr: String,
    },
}

impl DestructureAccessor {
    /// Register the Vapor runtime helper this accessor needs, if any.
    fn register_import(&self, out: &mut CodeGenOutput<'_>) {
        match self {
            DestructureAccessor::Path(_) => {}
            DestructureAccessor::Rest { .. } => out.add_vapor_import(VaporHelper::GetRestElement),
            DestructureAccessor::Default { .. } => {
                out.add_vapor_import(VaporHelper::GetDefaultValue)
            }
        }
    }

    /// Render the final accessor expression against a shared `base`.
    fn render(&self, base: &str) -> String {
        match self {
            DestructureAccessor::Path(suffix) => format!("{base}{suffix}"),
            DestructureAccessor::Rest {
                suffix,
                excluded_keys,
            } => {
                let mut keys = String::with_capacity(excluded_keys.len() * 8);
                for (i, key) in excluded_keys.iter().enumerate() {
                    if i > 0 {
                        keys.push_str(", ");
                    }
                    keys.push('"');
                    helpers::escape_js_string_into(&mut keys, key);
                    keys.push('"');
                }
                format!("_getRestElement({base}{suffix}, [{keys}])")
            }
            DestructureAccessor::Default {
                suffix,
                resolved_default_expr,
            } => {
                format!("_getDefaultValue({base}{suffix}, () => ({resolved_default_expr}))")
            }
        }
    }
}

/// Per-leaf-identifier accessor for a destructuring pattern (`{ id, name }`,
/// `[a, b]`, a rest element, a default value, arbitrary nesting/shorthand/
/// renamed-key/string-literal-key mixes), walked off an already-parsed OXC
/// AST — mirrors official `parseValueDestructure` (rc.5
/// `packages/compiler-vapor/src/transforms/vFor.ts`, confirmed directly
/// against the vendored dist). Returns `false` only for a construct official
/// ALSO doesn't destructure structurally (a computed key, or a rest/default
/// target that is itself a nested pattern) — the caller then leaves the
/// pattern's raw authored text as the closure's own param (still valid JS,
/// just not official's `_for_item{depth}`/`_slotProps{depth}`-renamed form).
fn collect_destructure_paths(
    expr: &Expression<'_>,
    path: &str,
    source: &str,
    resolver: &BindingResolver<'_>,
    out: &mut Vec<(String, DestructureAccessor)>,
) -> bool {
    use oxc_span::GetSpan;
    match expr {
        Expression::Identifier(id) => {
            out.push((
                id.name.as_str().to_string(),
                DestructureAccessor::Path(path.to_string()),
            ));
            true
        }
        Expression::ParenthesizedExpression(p) => {
            collect_destructure_paths(&p.expression, path, source, resolver, out)
        }
        Expression::AssignmentExpression(assign) => {
            // Default value (`id = 99`) in a destructuring position — the
            // v-for LHS parses leniently as a plain expression (not a
            // formal `BindingPattern`), so a default surfaces as an
            // `AssignmentExpression` here. A nested-pattern default
            // (`{ a } = {}`) is unsupported; only a bare identifier target is.
            let AssignmentTarget::AssignmentTargetIdentifier(target_id) = &assign.left else {
                return false;
            };
            let default_span = assign.right.span();
            let default_text = source
                .get(default_span.start as usize..default_span.end as usize)
                .unwrap_or_default();
            let resolved_default_expr = resolver.resolve_simple_expr(default_text);
            out.push((
                target_id.name.as_str().to_string(),
                DestructureAccessor::Default {
                    suffix: path.to_string(),
                    resolved_default_expr,
                },
            ));
            true
        }
        Expression::ObjectExpression(obj) => {
            // A rest element must be syntactically last, so every sibling
            // key seen before it in this loop is already the complete
            // exclusion list official's `_getRestElement` needs.
            let mut declared_keys: Vec<String> = Vec::new();
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if p.computed {
                            return false;
                        }
                        let mut child = String::with_capacity(path.len() + 8);
                        child.push_str(path);
                        let key_name = match &p.key {
                            PropertyKey::StaticIdentifier(name) => {
                                child.push('.');
                                child.push_str(name.name.as_str());
                                name.name.as_str().to_string()
                            }
                            PropertyKey::StringLiteral(s) => {
                                child.push_str("[\"");
                                helpers::escape_js_string_into(&mut child, s.value.as_str());
                                child.push_str("\"]");
                                s.value.as_str().to_string()
                            }
                            _ => return false,
                        };
                        declared_keys.push(key_name);
                        if !collect_destructure_paths(&p.value, &child, source, resolver, out) {
                            return false;
                        }
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        let Expression::Identifier(rest_id) = &spread.argument else {
                            return false; // nested rest target, unsupported
                        };
                        out.push((
                            rest_id.name.as_str().to_string(),
                            DestructureAccessor::Rest {
                                suffix: path.to_string(),
                                excluded_keys: declared_keys.clone(),
                            },
                        ));
                    }
                }
            }
            true
        }
        Expression::ArrayExpression(arr) => {
            let mut idx = 0usize;
            for elem in &arr.elements {
                match elem {
                    ArrayExpressionElement::Elision(_) => idx += 1,
                    ArrayExpressionElement::SpreadElement(_) => return false,
                    _ => {
                        let Some(e) = elem.as_expression() else {
                            return false;
                        };
                        let mut child = String::with_capacity(path.len() + 8);
                        child.push_str(path);
                        child.push('[');
                        push_usize(&mut child, idx);
                        child.push(']');
                        if !collect_destructure_paths(e, &child, source, resolver, out) {
                            return false;
                        }
                        idx += 1;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

/// `usize` decimal writer — `super::shared::helpers::push_u32` is `u32`-only.
fn push_usize(buf: &mut String, n: usize) {
    use std::fmt::Write as _;
    let _ = write!(buf, "{n}");
}

/// Leaf-name → accessor list for a v-for VALUE position's destructuring
/// pattern, or `None` if it isn't an object/array pattern at all (bare
/// identifier — handled by the existing simple-ident path) or hits an
/// unsupported construct. Official only destructures the value position —
/// key/index stay identifier-only ([`collect_destructure_paths`]'s doc).
fn destructure_value_paths(
    expr: &Expression<'_>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> Option<Vec<(String, DestructureAccessor)>> {
    if !matches!(
        expr,
        Expression::ObjectExpression(_) | Expression::ArrayExpression(_)
    ) {
        return None;
    }
    let mut out = Vec::new();
    if collect_destructure_paths(expr, "", source, resolver, &mut out) {
        Some(out)
    } else {
        None
    }
}

/// The v-for value (item) position's own expression — first element of a
/// parenthesized/sequence left-hand side (`(pattern, key) in …`), or the
/// bare expression itself for a single-position `v-for="item in items"`.
fn for_value_expr<'r, 'e>(left: &'r Expression<'e>) -> &'r Expression<'e> {
    let mut cur = left;
    while let Expression::ParenthesizedExpression(p) = cur {
        cur = &p.expression;
    }
    match cur {
        Expression::SequenceExpression(seq) => seq.expressions.first().unwrap_or(cur),
        other => other,
    }
}

/// The OXC-parsed v-for value pattern, if this element has a v-for with a
/// successfully-parsed left-hand side. `None` for no v-for / no OXC data /
/// a parse failure — callers fall back to the pre-existing raw-text path.
fn v_for_value_pattern<'r, 'e>(
    oxc_el: Option<&'r OxcParsedElement<'e>>,
) -> Option<&'r Expression<'e>> {
    let left = oxc_el?.v_for.as_ref()?.parsed.result.left.as_ref()?;
    Some(for_value_expr(left))
}

/// Per-leaf-identifier accessor for a v-slot scoped-slot param's
/// destructuring pattern, walked off the OXC-parsed `BindingPattern` — the
/// v-slot counterpart of [`collect_destructure_paths`] (same rules:
/// shorthand/renamed/nested keys, string-literal keys, rest elements, and
/// default values all resolve; only a computed key or a nested-pattern
/// rest/default target bails to `false`, leaving the caller's raw authored
/// text as the closure's own param). Confirmed against the pinned oracle:
/// `_getRestElement`/`_getDefaultValue` are the SAME helpers v-for uses,
/// just against a `_slotProps{depth}` base instead of `_for_item{depth}.value`.
fn collect_binding_destructure_paths(
    pattern: &BindingPattern<'_>,
    path: &str,
    source: &str,
    resolver: &BindingResolver<'_>,
    out: &mut Vec<(String, DestructureAccessor)>,
) -> bool {
    use oxc_span::GetSpan;
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            out.push((
                id.name.as_str().to_string(),
                DestructureAccessor::Path(path.to_string()),
            ));
            true
        }
        BindingPattern::ObjectPattern(obj) => {
            // Sibling keys declared before a rest element are its complete
            // exclusion list — same reasoning as `collect_destructure_paths`.
            let mut declared_keys: Vec<String> = Vec::new();
            for prop in &obj.properties {
                if prop.computed {
                    return false;
                }
                let mut child = String::with_capacity(path.len() + 8);
                child.push_str(path);
                let key_name = match &prop.key {
                    PropertyKey::StaticIdentifier(name) => {
                        child.push('.');
                        child.push_str(name.name.as_str());
                        name.name.as_str().to_string()
                    }
                    PropertyKey::StringLiteral(s) => {
                        child.push_str("[\"");
                        helpers::escape_js_string_into(&mut child, s.value.as_str());
                        child.push_str("\"]");
                        s.value.as_str().to_string()
                    }
                    _ => return false,
                };
                declared_keys.push(key_name);
                if !collect_binding_destructure_paths(&prop.value, &child, source, resolver, out) {
                    return false;
                }
            }
            if let Some(rest) = &obj.rest {
                let BindingPattern::BindingIdentifier(rest_id) = &rest.argument else {
                    return false; // nested rest target, unsupported
                };
                out.push((
                    rest_id.name.as_str().to_string(),
                    DestructureAccessor::Rest {
                        suffix: path.to_string(),
                        excluded_keys: declared_keys,
                    },
                ));
            }
            true
        }
        BindingPattern::ArrayPattern(arr) => {
            if arr.rest.is_some() {
                return false;
            }
            for (idx, elem) in arr.elements.iter().enumerate() {
                let Some(elem_pattern) = elem else {
                    continue; // elision/hole
                };
                let mut child = String::with_capacity(path.len() + 8);
                child.push_str(path);
                child.push('[');
                push_usize(&mut child, idx);
                child.push(']');
                if !collect_binding_destructure_paths(elem_pattern, &child, source, resolver, out) {
                    return false;
                }
            }
            true
        }
        BindingPattern::AssignmentPattern(assign) => {
            let BindingPattern::BindingIdentifier(target_id) = &assign.left else {
                return false; // nested-pattern default, unsupported
            };
            let default_span = assign.right.span();
            let default_text = source
                .get(default_span.start as usize..default_span.end as usize)
                .unwrap_or_default();
            let resolved_default_expr = resolver.resolve_simple_expr(default_text);
            out.push((
                target_id.name.as_str().to_string(),
                DestructureAccessor::Default {
                    suffix: path.to_string(),
                    resolved_default_expr,
                },
            ));
            true
        }
    }
}

/// The v-slot scoped-slot destructure leaves, if this `<template
/// v-slot="…">` wrapper's own param is an object/array PATTERN (never a
/// bare identifier — official only enters a fresh scope for a pattern; a
/// bare identifier keeps the pre-existing, already-correct raw-passthrough
/// behavior) AND every leaf resolves ([`collect_binding_destructure_paths`]
/// — a computed key or nested-pattern rest/default target bails the whole
/// element to `None`, same conservative fallback as the v-for value
/// position).
fn v_slot_destructure_leaves<'e>(
    oxc_el: Option<&OxcParsedElement<'e>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> Option<Vec<(String, DestructureAccessor)>> {
    let params = oxc_el?.v_slot.as_ref()?.parsed.params()?;
    if params.items.len() != 1 {
        return None;
    }
    let pattern = &params.items[0].pattern;
    if !matches!(
        pattern,
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_)
    ) {
        return None;
    }
    let mut out = Vec::new();
    if collect_binding_destructure_paths(pattern, "", source, resolver, &mut out) {
        Some(out)
    } else {
        None
    }
}

/// Loop-variable rename map for [`BindingResolver::push_for_scope`] —
/// official `itemVar = _for_item${depth}` + `buildDestructureIdMap` (rc.5).
/// `param_part` is [`helpers::parse_v_for_expression`]'s first return
/// (parens stripped); positions are value → key → index
/// ([`helpers::split_v_for_params`]).
///
/// A bare identifier is always renamed. The VALUE position (index 0) also
/// renames when it's a destructuring pattern [`destructure_value_paths`]
/// can fully resolve — each leaf identifier maps to
/// `_for_item{depth}.value<path>`. Key/index positions stay identifier-only
/// (official doesn't destructure them). `_` never gets an entry.
fn build_for_scope_map(
    param_part: &str,
    depth: u32,
    value_pattern: Option<&Expression<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
    out: &mut CodeGenOutput<'_>,
) -> rustc_hash::FxHashMap<String, String> {
    use super::binding::is_simple_ident;
    use super::shared::helpers::{push_u32, split_v_for_params};

    let mut map = rustc_hash::FxHashMap::default();
    let parts = split_v_for_params(param_part);
    let prefixes = ["_for_item", "_for_key", "_for_index"];
    for (i, (part, prefix)) in parts.iter().zip(prefixes.iter()).enumerate() {
        let Some(name) = part.map(str::trim) else {
            continue;
        };
        if name.is_empty() || name == "_" {
            continue;
        }
        let mut accessor_base = String::with_capacity(prefix.len() + 10);
        accessor_base.push_str(prefix);
        push_u32(&mut accessor_base, depth);
        accessor_base.push_str(".value");

        if is_simple_ident(name) {
            map.insert(name.to_string(), accessor_base);
            continue;
        }
        if i == 0 {
            if let Some(leaves) =
                value_pattern.and_then(|p| destructure_value_paths(p, source, resolver))
            {
                for (leaf_name, accessor) in leaves {
                    accessor.register_import(out);
                    map.insert(leaf_name, accessor.render(&accessor_base));
                }
            }
        }
    }
    map
}

/// Main-closure params: renamed `_for_item{depth}`… for each bare
/// identifier, or for the VALUE position when it's a destructuring pattern
/// [`destructure_value_paths`] can fully resolve — same eligibility as
/// [`build_for_scope_map`], computed independently since the two run at
/// different points in the walk (enter vs. leave). A position this pass
/// can't destructure stays as authored text (still valid JS).
fn build_for_callback_params(
    param_part: &str,
    depth: u32,
    value_pattern: Option<&Expression<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> String {
    use super::binding::is_simple_ident;
    use super::shared::helpers::{push_u32, split_v_for_params};

    let parts = split_v_for_params(param_part);
    let prefixes = ["_for_item", "_for_key", "_for_index"];
    let mut pieces: Vec<String> = Vec::with_capacity(3);
    for (i, (part, prefix)) in parts.iter().zip(prefixes.iter()).enumerate() {
        let Some(name) = part.map(str::trim) else {
            break;
        };
        let renames = (is_simple_ident(name) && name != "_")
            || (i == 0
                && value_pattern
                    .and_then(|p| destructure_value_paths(p, source, resolver))
                    .is_some());
        if renames {
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
    /// plain root wrapper (rc.5 `basic-interpolation.vue`).
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
    /// Each entry is `(template_idx, html, is_static, is_root)`. `is_static`
    /// is official `canUseStaticTemplate()` (no effects/nav/text-extractions/
    /// statements). `is_root` mirrors official `isSingleRootChild` — the
    /// template-wide root bit propagates through v-if/v-else-if/v-else
    /// branches (never v-for, never a component/slot boundary) exactly when
    /// the whole chain IS the SFC's sole meaningful top-level construct
    /// (`self.template_single_root`); every other closure (v-for body,
    /// slot/fallback/default-slot content) stays `false`.
    hoisted_templates: Vec<(u32, String, bool, bool)>,
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
    /// (rc.5). Verter's bottom-up walker would otherwise consume a child
    /// interpolation id before `leave_element`.
    pending_construct_ref: Vec<Option<u32>>,
    /// Whether the whole template has exactly one meaningful top-level
    /// construct (official `hasSingleRootChild`/`isSingleRoot` at the
    /// template root) — a v-else-if/v-else continuation doesn't count as a
    /// separate root, mirroring `vdom::VdomCodeGen::single_root`. Computed
    /// once in `enter_template`, before the walk assigns any node ids.
    template_single_root: bool,
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
            template_single_root: false,
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
    ///
    /// `is_root`: official `isSingleRootChild` propagated into this closure's
    /// own hoisted template — `true` only for a v-if/v-else-if/v-else branch
    /// whose whole chain IS the template's sole meaningful top-level
    /// construct (`self.template_single_root`); `false` for every other
    /// caller (v-for body, slot/fallback/default-slot content — a
    /// component/slot boundary always breaks the propagation).
    fn build_closure_body(
        &mut self,
        mut state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        indent: &str,
        is_root: bool,
        out: &mut CodeGenOutput<'alloc>,
    ) -> (String, bool, Vec<SegmentAnchor>) {
        use super::shared::helpers::push_u32;
        let mut anchors: Vec<SegmentAnchor> = Vec::new();

        // A transparent `<template v-if>`/`<template v-for>` wrapper whose
        // sole meaningful content is itself a structural construct (nested
        // v-if/v-for/component/slot) owns no DOM container or template of
        // its own — `merge_into_stack_index` already donated the child's
        // own ref as `state.node_ref` and its statement as
        // `state.child_statements`. This scope's body is exactly that
        // statement, verbatim; no template registration, no `tN()`
        // instantiation.
        if state.donated_construct {
            let node_ref = state
                .node_ref
                .expect("donation always sets node_ref before build_closure_body");
            let is_static = state.own_effects.is_empty()
                && state.child_effects.is_empty()
                && state.child_nav.is_empty()
                && state.child_text_creations.is_empty()
                && state.text_node_ref.is_none()
                && state.child_statements.is_empty();
            let mut body = String::with_capacity(64);
            for (stmt, stmt_anchors) in &state.child_statements {
                body.push_str(indent);
                body.push_str("  ");
                push_body_with_anchors(&mut body, stmt, stmt_anchors, &mut anchors);
                body.push('\n');
            }
            body.push_str(indent);
            body.push_str("  return n");
            push_u32(&mut body, node_ref);
            body.push('\n');
            return (body, is_static, anchors);
        }

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

        self.hoisted_templates.push((
            template_idx,
            std::mem::take(&mut state.html),
            is_static,
            is_root,
        ));

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
        // `_child`/`_next` are imported per the nav content actually
        // emitted (see `element::finalize_root_element`'s matching comment)
        // — a single-dynamic-child closure never calls `_next` at all.
        if state.child_nav.iter().any(|nav| nav.contains("_child(")) {
            out.add_vapor_import(VaporHelper::Child);
        }
        if state.child_nav.iter().any(|nav| nav.contains("_next(")) {
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
        // runtime `ReferenceError` — see `element::finalize_root_element`. A
        // nav-chain ref (`!text_ref_generated`, mixed-content container) needs
        // no extraction line: its establishment is already in `child_nav` and
        // its `_renderEffect` statement is already in `child_statements`
        // (`resolve_pending_nav_requests`'s `TextRef` arm).
        let own_text_needs_extraction = state.text_node_ref.is_some() && state.text_ref_generated;
        if own_text_needs_extraction {
            body.push_str(indent);
            body.push_str("  const x");
            push_u32(&mut body, state.text_node_ref.expect("checked Some above"));
            body.push_str(" = _txt(n");
            push_u32(&mut body, inner_ref);
            body.push_str(")\n");
        }
        if !state.child_text_creations.is_empty() || own_text_needs_extraction {
            out.add_vapor_import(VaporHelper::Txt);
        }
        if !state.child_text_creations.is_empty() || state.text_node_ref.is_some() {
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
            // Official `processRepeatedVariables`: a `_ctx.` read repeated
            // within this render effect hoists to a local `const` — forces
            // the braced form even for what would otherwise be a single
            // concise-arrow effect.
            let hoisted = repeated_reads::hoist_repeated_ctx_reads(&mut all_effects, out);
            body.push_str(indent);
            body.push_str("  _renderEffect(() => ");
            if hoisted.is_empty() && all_effects.len() == 1 {
                all_effects[0].write_code_into_with_anchors(&mut body, &mut anchors);
            } else {
                body.push_str("{\n");
                for (decl_text, decl_anchors) in &hoisted {
                    body.push_str(indent);
                    body.push_str("    ");
                    push_body_with_anchors(&mut body, decl_text, decl_anchors, &mut anchors);
                    body.push('\n');
                }
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
        // construct id + one wasted branch-entry id before branch content (rc.5);
        // leave-time reservation is too late for this bottom-up walker.
        let outer_ref = construct_ref;
        // Official `isSingleRootChild`: propagates through v-if/v-else-if/
        // v-else branches (never v-for) exactly when this whole chain is the
        // template's sole meaningful top-level construct. `self.depth` is
        // already decremented to this branch element's OWN level (`leave_element`
        // decrements before dispatching here) — 0 means this branch sits
        // directly at the template root, not nested inside another element.
        let is_root = self.template_single_root && self.depth == 0;
        let (body, is_static, body_anchors) =
            self.build_closure_body(state, has_dynamic_text, "  ", is_root, out);

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
            own_text_ref_generated: true,
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
                // explicit `null` placeholder for the skipped 3rd arg (rc.5 `genCall`).
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
        oxc_el: Option<&OxcParsedElement<'alloc>>,
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
                own_text_ref_generated: true,
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
            self.build_closure_body(state, has_dynamic_text, "  ", false, out);

        // Extract :key expression if present
        let key_expr = self.extract_key_expr(el, source);

        // Build the _createFor statement
        let resolved_source = self.resolve_v_for_source(source_part);
        // Main-closure params use renamed `_for_item{depth}`… (`itemVar`/`keyVar`/
        // `indexVar`). `for_scope_depth` here already matches this v-for's enter
        // depth (pop runs in `leave_element` before this function).
        let value_pattern = v_for_value_pattern(oxc_el);
        let for_callback_params = build_for_callback_params(
            param_part,
            self.for_scope_depth,
            value_pattern,
            source,
            &self.resolver,
        );
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
        // (rc.5: `(item) => (item)`, never `(_for_item0) => (_for_item0.value)`).
        let has_key = key_expr.is_some();
        if let Some(key) = key_expr {
            stmt.push_str(", (");
            stmt.push_str(param_part);
            stmt.push_str(") => (");
            stmt.push_str(key);
            stmt.push(')');
        }

        // `_createFor` 4th arg. Flags-present + key-absent needs an explicit
        // `undefined` key-slot placeholder (rc.5:
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
            own_text_ref_generated: true,
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
    /// single-arg `next(node) => node.nextSibling` (rc.5). Separate from
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
    /// into its parent. Official rc.5 is not limited to v-if/v-for: a component
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
        kind: MergedConstructKind,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Callers merge immediately after `id`'s stack entry was popped; last
        // is the parent. `merge_vif_chain_into_target` is the exception.
        let Some(parent_index) = self.element_stack.len().checked_sub(1) else {
            return;
        };
        self.merge_into_stack_index(parent_index, id, root, kind, source, out);
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
        // A v-if/v-else chain is never NON_STABLE-forwarding-eligible
        // (official `markSlotRootIf` is a distinct, unimplemented branch —
        // out of this pass's scope).
        self.merge_into_stack_index(
            target_index,
            id,
            root,
            MergedConstructKind::Other,
            source,
            out,
        );
    }

    /// Shared merge body; callers differ only in which `element_stack` index
    /// they target.
    fn merge_into_stack_index(
        &mut self,
        target_index: usize,
        id: NodeId,
        root: VaporRootElement<'alloc>,
        kind: MergedConstructKind,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        let dom_child_index = self
            .element_stack
            .get_mut(target_index)
            .map(|parent| parent.observe_dom_element())
            .unwrap_or(0);
        let has_following = self.has_following_template_contributing_sibling(id, source);

        // A transparent `<template v-if>`/`<template v-for>` target
        // (`is_transparent_wrapper`) owns no DOM container to
        // `_setInsertionState`/`_child`/`_next` into — official Vue never
        // builds one either. When this merged construct is the target's
        // SOLE meaningful content (`dom_child_index == 0 && !has_following`,
        // nothing donated yet), donate it directly: the construct's own
        // statement already declares and returns its ref, so that ref
        // simply BECOMES the target's own `node_ref` and the statement
        // becomes the target's own body — no nav, no insertion state, no
        // second template. A non-sole structural child under a transparent
        // wrapper (a mixed-sibling multi-root `<template v-if>`) is a
        // distinct, larger feature (official's `TRUE_MULTI_ROOT` array
        // return) and stays on the nav-based path below, which requires a
        // container this wrapper doesn't have — not yet supported.
        let donate = dom_child_index == 0
            && !has_following
            && self
                .element_stack
                .get(target_index)
                .is_some_and(|parent| parent.is_transparent_wrapper && parent.node_ref.is_none());
        if donate {
            if let Some(parent) = self.element_stack.get_mut(target_index) {
                parent.merged_construct_kinds.push((kind, root.node_ref));
                parent.node_ref = Some(root.node_ref);
                parent.donated_construct = true;
                parent.child_nav.extend(root.nav);
                parent.child_text_creations.extend(root.text_creations);
                parent.child_effects.extend(root.effects);
                if root.own_text_ref.is_some() {
                    parent.text_node_ref = root.own_text_ref;
                    parent.text_ref_generated = root.own_text_ref_generated;
                }
                for stmt in root.statements {
                    parent.child_statements.push(stmt);
                }
            }
            return;
        }

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
            parent.merged_construct_kinds.push((kind, root.node_ref));
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

    /// Official 2-arg `setInsertionState(parent, anchor)` (rc.5) — append
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
    /// (rc.5: `increaseId()` for the anchor precedes memoized `reference()`).
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
                PendingNavRequest::TextRef {
                    own_ref,
                    nav_slot,
                    stmt_slot,
                } => {
                    let container_ref = state.ensure_node_ref(&mut self.counters);
                    self.emit_chained_nav(
                        own_ref,
                        container_ref,
                        &mut chain,
                        &mut state.child_nav,
                        nav_slot,
                        out,
                    );
                    self.emit_interleaved_text_effect(own_ref, state, stmt_slot, out);
                }
            }
        }
    }

    /// Build this text run's `_renderEffect(...)` statement and write it
    /// into its reserved `child_statements` slot — official interleaves it
    /// at the run's own DFS position (`flushBeforeDynamic`) instead of
    /// deferring it to the block's aggregated effect list. Mirrors
    /// `build_closure_body`'s concise-vs-braced choice (single effect, no
    /// hoisted repeated `_ctx.` reads → concise arrow).
    fn emit_interleaved_text_effect(
        &mut self,
        own_ref: u32,
        state: &mut VaporElementState<'alloc>,
        stmt_slot: usize,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let parts = std::mem::take(&mut state.text_parts);
        let mut effects = vec![VaporEffect::SetText {
            text_ref: own_ref,
            parts,
            generated: false,
        }];
        let hoisted = repeated_reads::hoist_repeated_ctx_reads(&mut effects, out);

        let mut stmt = String::with_capacity(64);
        let mut stmt_anchors: Vec<SegmentAnchor> = Vec::new();
        stmt.push_str("_renderEffect(() => ");
        if hoisted.is_empty() {
            effects[0].write_code_into_with_anchors(&mut stmt, &mut stmt_anchors);
        } else {
            stmt.push_str("{\n");
            for (decl_text, decl_anchors) in &hoisted {
                stmt.push_str("    ");
                push_body_with_anchors(&mut stmt, decl_text, decl_anchors, &mut stmt_anchors);
                stmt.push('\n');
            }
            for effect in &effects {
                stmt.push_str("    ");
                effect.write_code_into_with_anchors(&mut stmt, &mut stmt_anchors);
                stmt.push('\n');
            }
            stmt.push_str("  }");
        }
        stmt.push(')');
        out.add_vapor_import(VaporHelper::RenderEffect);
        state.child_statements[stmt_slot] = (
            out.alloc_str(&stmt),
            out.alloc_segment_anchors(&stmt_anchors),
        );
    }

    /// Official `processDynamicChildren`'s "reusable `p*` cursor" rule: a
    /// dynamic child (component/slot outlet/v-if/v-for) needs a REAL `<!>`
    /// comment anchor + chained nav only when a TEMPLATE-CONTRIBUTING
    /// sibling — plain static markup, a text/interpolation run, or a
    /// rendered comment — still follows it. Another dynamic construct
    /// contributes no template content of its own, so it doesn't count:
    /// once nothing template-contributing remains, every further dynamic
    /// child is a bare numeric `_setInsertionState(container, index)` with
    /// no comment marker at all (`lastTemplateIndex` in the official
    /// source — every dynamic child at or after it skips the anchor).
    fn has_following_template_contributing_sibling(&self, id: NodeId, source: &str) -> bool {
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
        siblings[pos + 1..].iter().any(|&sib| {
            self.is_meaningful_sibling(sib, source) && self.is_template_contributing(sib)
        })
    }

    /// Whether a sibling writes directly into the parent's own static
    /// template — a plain element with no `v-if`/`v-for`, a rendered
    /// comment, or text/interpolation. A component, slot outlet, `<template
    /// v-slot>`, or any `v-if`/`v-for`-wrapped element never contributes to
    /// the parent's template (it always renders through the block/
    /// insertion-state mechanism instead), so it does not count.
    fn is_template_contributing(&self, id: NodeId) -> bool {
        match &self.ast.nodes[id.0].kind {
            AstNodeKind::Element(el) => {
                el.v_condition.is_none() && el.v_for.is_none() && el.tag_type == TagType::Element
            }
            AstNodeKind::Text(_) | AstNodeKind::Interpolation(_) => true,
            AstNodeKind::Comment(_) => self.options.comments,
        }
    }

    /// Whether AST node `id` is an Element or a Comment — vdom
    /// `element::is_element_or_comment`'s own whitespace-condense check,
    /// mirrored here for vapor's streaming walk (vdom builds a full
    /// `Vec<ChildRecord>` first and resolves whitespace in one pass; vapor
    /// visits one child at a time, so `visit_text` asks per-node instead).
    fn is_element_or_comment(&self, id: NodeId) -> bool {
        matches!(
            self.ast.nodes[id.0].kind,
            AstNodeKind::Element(_) | AstNodeKind::Comment(_)
        )
    }

    /// Official `condenseWhitespace`: an INTERIOR whitespace-only text node
    /// containing a newline is dropped only when BOTH neighbors are
    /// element/comment; otherwise it collapses to a single space instead of
    /// vanishing (e.g. between a `<slot>` and a following interpolation —
    /// `components/child-comp.vue`'s `</slot>\n{{ label }}`). A LEADING or
    /// TRAILING whitespace-newline (no previous/next sibling) is always
    /// dropped, matching vdom `resolve_whitespace`'s separate leading/
    /// trailing trim step (mirrored here, not shared — see
    /// `is_element_or_comment`'s doc).
    fn whitespace_newline_collapses_to_space(&self, id: NodeId) -> bool {
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
        if pos == 0 || pos + 1 == siblings.len() {
            return false;
        }
        !(self.is_element_or_comment(siblings[pos - 1])
            && self.is_element_or_comment(siblings[pos + 1]))
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

    /// Whether `id` is the ONLY meaningful child of its parent (any kind of
    /// parent) — unlike [`Self::v_for_is_only_child`], not restricted to a
    /// plain-element parent. Used to gate the "this plain element IS its
    /// component-scope parent's own root" merge: a parent with more than one
    /// meaningful child is a different (unimplemented) multi-root-slot
    /// shape, not this narrow single-child case.
    fn is_sole_meaningful_child(&self, id: NodeId, source: &str) -> bool {
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
        siblings
            .iter()
            .filter(|&&sib| self.is_meaningful_sibling(sib, source))
            .count()
            == 1
    }

    /// Official FAST_REMOVE source: `isOnlyChild = parent &&
    /// parent.block.node !== parent.node && parent.node.children.length === 1`
    /// (rc.5 `processFor`). Sole meaningful child of a PLAIN parent — not a
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
        state: VaporElementState<'alloc>,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::{is_builtin_component, push_u32, to_pascal_case};

        // `<component :is="expr">` — official Vapor routes this through
        // `_createDynamicComponent(() => (expr), props, slots)`, a
        // dedicated helper distinct from `_createComponent`'s
        // statically-resolved-reference path below. Detected FIRST since
        // `component` is never itself a direct/PascalCase/built-in binding.
        let dynamic_is = resolve_vapor_dynamic_component(
            el,
            tag_name,
            source,
            oxc_el,
            &self.resolver,
            self.options.force_js,
        );
        if let Some((is_expr, _is_prop_idx)) = dynamic_is {
            return self
                .build_dynamic_component_root(el, is_expr, node_ref, source, state, oxc_el, out);
        }

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
            } else if matches!(tag_name, "Teleport" | "teleport")
                || matches!(pascal.as_str(), "Teleport")
            {
                // Vapor has its OWN Teleport/KeepAlive runtime components —
                // `_VaporTeleport`/`_VaporKeepAlive`, imported from `"vue"`
                // like any other Vapor helper, never the VDOM `_Teleport`/
                // `_KeepAlive` names `is_builtin_component` returns.
                out.add_vapor_import(VaporHelper::VaporTeleport);
                (None, VaporHelper::VaporTeleport.name())
            } else if matches!(tag_name, "KeepAlive" | "keep-alive")
                || matches!(pascal.as_str(), "KeepAlive")
            {
                out.add_vapor_import(VaporHelper::VaporKeepAlive);
                (None, VaporHelper::VaporKeepAlive.name())
            } else if let Some((flag, helper_name)) =
                is_builtin_component(tag_name).or_else(|| is_builtin_component(&pascal))
            {
                // Other Vue built-ins (Transition, Suspense, …) — not yet
                // ported to Vapor-specific names; unchanged for now.
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
        let props_str = self.build_component_props(el, source, oxc_el, false, out);

        // Build slot closures from children
        let raw_children = super::shared::helpers::is_raw_children_builtin(tag_name);
        let slots_str = self.build_component_slots(state, el, raw_children, out);

        let mut create_line = String::with_capacity(128);
        create_line.push_str("const n");
        push_u32(&mut create_line, node_ref);
        create_line.push_str(" = _createComponent(");
        create_line.push_str(comp_ref);
        create_line.push_str(", ");
        if let Some(props) = &props_str {
            create_line.push_str(props);
        } else {
            create_line.push_str("null");
        }
        create_line.push_str(", ");
        if let Some(slots) = &slots_str {
            create_line.push_str(slots);
        } else {
            create_line.push_str("null");
        }
        // Trailing `true` — official Vue's `isSingleRoot` marker, present
        // on every statically-resolved `_createComponent(...)` call
        // observed (with or without props/slots).
        create_line.push_str(", true)");
        out.add_vapor_import(VaporHelper::CreateComponent);

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
            own_text_ref_generated: true,
            statements,
            v_once: false,
            v_memo_expr: None,
        }
    }

    /// Build a component's slot closures from its accumulated child state
    /// (named `<template #x>` entries plus any implicit default content).
    /// Shared by [`Self::build_component_root`] (statically-resolved
    /// components) and [`Self::build_dynamic_component_root`]
    /// (`<component :is>`) — identical slot shape either way.
    /// `raw_children`: true for Teleport/KeepAlive (`is_raw_children_builtin`)
    /// — official Vapor passes their default content as a BARE `() => {
    /// ... }` closure, never the `{ default: ..., _: 2 }` slot-object form
    /// normal components use. Only applies to the no-named-slots case
    /// (Teleport/KeepAlive don't take named slots).
    fn build_component_slots(
        &mut self,
        mut state: VaporElementState<'alloc>,
        el: &ElementNode,
        raw_children: bool,
        out: &mut CodeGenOutput<'alloc>,
    ) -> Option<String> {
        let named_slots = std::mem::take(&mut state.named_slots);

        // Official `hasStableSlotRoot`/`markSlotRootOperations`: an implicit
        // default slot whose ENTIRE content is one non-stable construct (a
        // bare `<slot>` forward or a `<component :is>`, with no static HTML
        // at all) forwards through `_extend(() => {...}, { _: 8 /*
        // NON_STABLE */ })` directly as the slots argument — ahead of both
        // the raw-children (Teleport/KeepAlive) and named-slots forms below.
        if named_slots.is_empty() {
            if let Some(non_stable) = self.try_build_non_stable_slot_root(&mut state, out) {
                return Some(non_stable);
            }
        }

        let has_default_content = !state.html.is_empty()
            || !state.child_nav.is_empty()
            || !state.child_effects.is_empty()
            || !state.child_statements.is_empty()
            || !state.child_text_creations.is_empty();

        if raw_children && named_slots.is_empty() {
            return has_default_content.then(|| self.build_raw_children_closure(state, el, out));
        }

        if !named_slots.is_empty() {
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
                    self.build_closure_body(state, has_dynamic_text, "    ", false, out);
                if !named_slots.is_empty() {
                    result.push_str(", ");
                }
                result.push_str("\"default\": () => {\n");
                result.push_str(&body);
                result.push_str("    }");
            }
            result.push_str(" }");
            Some(result)
        } else if has_default_content {
            Some(self.build_default_slot_closure(state, el, out))
        } else {
            None
        }
    }

    /// Official `hasStableSlotRoot`/`markSlotRootOperations`, restricted to
    /// the narrow shape this backend can positively confirm: the implicit
    /// default slot's ENTIRE content is a single merged
    /// `SlotOutlet`/`DynamicComponent` construct, no static HTML, no other
    /// nav/effects/text extraction. That is never a "stable" root (a bare
    /// `<slot>` forward has no template of its own; a dynamic `<component
    /// :is>` isn't statically resolved), so it forwards as `_extend(() => {
    /// ...; return n{ref} }, { _: 8 /* NON_STABLE */ })` — the merged
    /// construct's OWN statements verbatim, no synthetic empty template and
    /// no `_setInsertionState` (there is no real DOM container to insert
    /// into). Returns `None` when the shape doesn't match; the caller falls
    /// through to its normal path.
    fn try_build_non_stable_slot_root(
        &mut self,
        state: &mut VaporElementState<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> Option<String> {
        use super::shared::helpers::push_u32;

        if !state.html.is_empty()
            || state.merged_construct_kinds.len() != 1
            || !state.child_nav.is_empty()
            || !state.child_effects.is_empty()
            || !state.child_text_creations.is_empty()
        {
            return None;
        }
        let (kind, child_ref) = state.merged_construct_kinds[0];
        if !matches!(
            kind,
            MergedConstructKind::SlotOutlet | MergedConstructKind::DynamicComponent
        ) {
            return None;
        }

        out.add_vapor_import(VaporHelper::Extend);
        let mut result = String::with_capacity(128);
        result.push_str("_extend(() => {\n");
        for (stmt, _anchors) in &state.child_statements {
            // The merge's reserved placeholder slot — a real DOM container's
            // `_setInsertionState(...)` this path never needs, so it was
            // never resolved.
            if stmt.is_empty() {
                continue;
            }
            result.push_str("    ");
            result.push_str(stmt);
            result.push('\n');
        }
        result.push_str("    return n");
        push_u32(&mut result, child_ref);
        result.push('\n');
        result.push_str("  }, { _: 8 /* NON_STABLE */ })");
        Some(result)
    }

    /// Build a `<component :is="expr">` root: `_createDynamicComponent(()
    /// => (expr), props, slots)`. Distinct helper from
    /// [`Self::build_component_root`]'s statically-resolved
    /// `_createComponent(...)` path — official Vapor never resolves a
    /// dynamic tag through `_resolveComponent`/a direct binding lookup.
    #[allow(clippy::too_many_arguments)]
    fn build_dynamic_component_root(
        &mut self,
        el: &ElementNode,
        is_expr: String,
        node_ref: u32,
        source: &'alloc str,
        state: VaporElementState<'alloc>,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        // This construct's OWN parent is still on the stack — `leave_element`
        // pops this element's entry before calling here.
        let is_sole_template_root = self.template_single_root && self.depth == 0;
        let is_slot_content_root = self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_component_scope);
        let props_str = self.build_component_props(el, source, oxc_el, true, out);
        let slots_str = self.build_component_slots(state, el, false, out);

        let mut create_line = String::with_capacity(128);
        create_line.push_str("const n");
        push_u32(&mut create_line, node_ref);
        create_line.push_str(" = _createDynamicComponent(() => (");
        create_line.push_str(&is_expr);
        create_line.push_str("), ");
        if let Some(props) = &props_str {
            create_line.push_str(props);
        } else {
            create_line.push_str("null");
        }
        create_line.push_str(", ");
        if let Some(slots) = &slots_str {
            create_line.push_str(slots);
        } else {
            create_line.push_str("null");
        }
        // Trailing block-topology flag: the sole template root gets
        // SINGLE_ROOT (1); a dynamic component nested inside another
        // component's slot content gets SLOT_ROOT (4) — verified against
        // `components/component-is` (root) and `built-ins/keep-alive`'s
        // inner dynamic child (slot content). A dynamic component that is
        // neither (e.g. one sibling of a multi-root template) gets no
        // trailing flag at all — verified against `components/dynamic-multi-root`.
        if is_sole_template_root {
            if self.options.is_production {
                create_line.push_str(", 1");
            } else {
                create_line.push_str(", 1 /* SINGLE_ROOT */");
            }
        } else if is_slot_content_root {
            if self.options.is_production {
                create_line.push_str(", 4");
            } else {
                create_line.push_str(", 4 /* SLOT_ROOT */");
            }
        }
        create_line.push(')');
        out.add_vapor_import(VaporHelper::CreateDynamicComponent);

        VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            own_text_ref: None,
            own_text_ref_generated: true,
            statements: vec![(out.alloc_str(&create_line), &[] as &[SegmentAnchor])],
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
            self.build_closure_body(state, has_dynamic_text, "    ", false, out);

        let mut result = String::with_capacity(128);
        result.push_str("{ default: () => {\n");
        result.push_str(&body);
        result.push_str("    }, _: 2 }");
        result
    }

    /// Build the bare `() => { ...; return n0 }` closure Teleport/KeepAlive
    /// take for their default content — no `{ default: ..., _: 2 }` slot
    /// object, see `build_component_slots`'s `raw_children` doc comment.
    fn build_raw_children_closure(
        &mut self,
        state: VaporElementState<'alloc>,
        el: &ElementNode,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
        let (body, _is_static, _body_anchors) =
            self.build_closure_body(state, has_dynamic_text, "  ", false, out);

        let mut result = String::with_capacity(128);
        result.push_str("() => {\n");
        result.push_str(&body);
        result.push('}');
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
        skip_is: bool,
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
                    if attr_name == "is" && skip_is {
                        continue; // <component :is> — consumed by the dynamic-component resolver, not a prop
                    }
                    let mut entry = String::with_capacity(32);
                    push_prop_key(&mut entry, attr_name);
                    entry.push_str(": ");
                    let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                    let resolved =
                        resolve_expr(value, vs, oxc_exp, &self.resolver, self.options.force_js);
                    // A pure-literal value (`:count="3"`) is constant — no
                    // reactive re-evaluation is ever needed, so official
                    // Vue emits the bare value rather than a `() => (...)`
                    // lazy getter (verified against the real compiler).
                    let is_pure_literal = oxc_exp.is_some_and(|e| {
                        e.dynamism == Dynamism::Static
                            && e.bindings
                                .as_ref()
                                .is_some_and(|b| b.non_ignored_binding_names().is_empty())
                    });
                    if is_pure_literal {
                        entry.push_str(&resolved);
                    } else {
                        entry.push_str("() => (");
                        entry.push_str(&resolved);
                        entry.push(')');
                    }
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

    /// Named `:prop="expr"` / static-attribute slot-outlet props — official
    /// `genRawProps`/`genProp` (rc.5 `generators/slotOutlet.ts` +
    /// `generators/props.ts`, confirmed against the vendored dist): a
    /// compile-time-constant value (string/number/boolean/`null`/
    /// `undefined` literal, or a static HTML attribute — always a source
    /// literal) is emitted directly; anything else — including a bare
    /// identifier reference like `count` — wraps in a getter (`() => (…)`)
    /// to preserve lazy/reactive access on read.
    ///
    /// A `v-bind` spread and a dynamic key (`:[key]="val"`) are OUT OF
    /// SCOPE and CONFIRMED WRONG, not merely non-conformant — official
    /// routes ALL of static/dynamic-key/spread props through one shared
    /// `{ $: [fn, ...] }` merge-array form (`genRawProps`'s
    /// `PropsExpression::MergeExpression`, confirmed against the vendored
    /// dist and directly probed against the pinned oracle); this function
    /// builds only the flat-object form:
    /// - `v-bind="expr"` (spread, no arg): the loop `continue`s before
    ///   contributing anything — the spread source is SILENTLY DROPPED.
    ///   `<slot v-bind="extra">` with `extra` the ONLY prop compiles to a
    ///   bare `_createSlot()` — `extra`'s data never reaches the slot at
    ///   all (confirmed via `scoped_slot_outlet_spread_is_silently_dropped`,
    ///   the same "prop silently vanishes" class the `<slot :total="count">`
    ///   fix elsewhere in this module closed once already).
    /// - `:[key]="val"` (dynamic key): `arg_start`/`arg_end` span the RAW
    ///   bracketed text (`"[key]"`), which this function then emits as a
    ///   literal STATIC prop key — a bogus prop named `"[key]"`, not the
    ///   intended runtime-computed key (confirmed via
    ///   `scoped_slot_outlet_dynamic_key_emits_wrong_literal_key`).
    ///
    /// Closing either needs the shared `$:` merge-array mechanism — a
    /// materially separate, larger feature (interleaving static/dynamic-key/
    /// spread sources in official's exact merge order), not a quick fix; a
    /// naive one-off spread (`{ ...expr }`) would trade this bug for a
    /// different one (losing the getter-wrapped lazy/reactive read official's
    /// form preserves). `v-model` on a `<slot>` outlet is not a construct
    /// Vue templates author (no fixture exercises it either).
    /// Returns `None` when no named prop is present.
    fn build_slot_outlet_props(
        &self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> Option<String> {
        let mut buf = String::new();
        let mut count = 0usize;
        for (prop_idx, prop) in el.props.iter().enumerate() {
            if !prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "name" {
                    continue;
                }
                if count > 0 {
                    buf.push_str(", ");
                }
                push_prop_key(&mut buf, name);
                buf.push_str(": ");
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    buf.push('"');
                    helpers::escape_js_string_into(&mut buf, &source[vs as usize..ve as usize]);
                    buf.push('"');
                } else {
                    buf.push_str("true");
                }
                count += 1;
                continue;
            }

            let dname = &source[prop.start as usize..prop.name_end as usize];
            if !super::vdom::is_v_bind(dname) {
                continue;
            }
            let Some((as_, ae)) = prop.arg_start.zip(prop.arg_end) else {
                continue; // bare `v-bind="expr"` spread — unsupported here
            };
            let arg = &source[as_ as usize..ae as usize];
            if arg == "name" {
                continue;
            }
            if count > 0 {
                buf.push_str(", ");
            }
            let key = super::vdom::props::camelize(arg);
            push_prop_key(&mut buf, &key);
            buf.push_str(": ");
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let raw = &source[vs as usize..ve as usize];
                let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                let resolved =
                    resolve_expr(raw, vs, oxc_exp, &self.resolver, self.options.force_js);
                if is_direct_constant_expr(raw.trim()) {
                    buf.push_str(&resolved);
                } else {
                    buf.push_str("() => (");
                    buf.push_str(&resolved);
                    buf.push(')');
                }
            } else {
                // Same-name shorthand: `:total` → `total: () => (resolved)`
                let resolved = self.resolver.resolve_simple_expr(&key);
                buf.push_str("() => (");
                buf.push_str(&resolved);
                buf.push(')');
            }
            count += 1;
        }
        if count == 0 {
            None
        } else {
            Some(format!("{{ {buf} }}"))
        }
    }

    /// `_createSlot(name, props, fallback)`, trailing default-valued args
    /// omitted (rc.5 `slots.vue`: named + fallback emits all three; bare
    /// `<slot />` emits `_createSlot()`).
    ///
    /// `state` is the outlet's fallback content (its own HTML scope, handed
    /// over via `take_scope_html`) and is built with `build_closure_body`,
    /// not discarded.
    fn build_slot_outlet_root(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        node_ref: u32,
        mut state: VaporElementState<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        // Slot name from static `name`; the generated literal's opening quote
        // anchors to the ATTRIBUTE name's start, not the value (rc.5
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
                self.build_closure_body(state, has_dynamic_text, "  ", false, out);
            let mut closure = String::with_capacity(64 + body.len());
            closure.push_str("() => {\n");
            closure.push_str(&body);
            closure.push_str("  }");
            Some(closure)
        } else {
            None
        };

        // A bare `<slot>` forward that is the ENTIRE content of a
        // component's implicit default slot (this construct's immediate
        // parent — still on the stack, `leave_element` pops its own entry
        // before calling here — has no real DOM container) is never a
        // stable slot root (official `markSlotRootOperations`'s type-12
        // branch): SLOT_ROOT (4) always, INHERIT_FALLBACK (32) since this
        // backend does not yet detect the SHARED_FALLBACK (v-for-forced)
        // case. `try_build_non_stable_slot_root` decides whether the WHOLE
        // slot ends up NON_STABLE-forwarded; this only needs to know
        // whether trailing flags apply to ITS OWN `_createSlot(...)` call.
        let is_slot_content_root = self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_component_scope);
        let flags: u32 = if is_slot_content_root { 4 | 32 } else { 0 };

        // Omit trailing defaults: fallback, then props (`null`), then name (`"default"`).
        let props_arg = self.build_slot_outlet_props(el, oxc_el, source);
        let props_is_default = props_arg.is_none();
        let name_is_default = slot_name == "default";
        let name_arg_included =
            fallback.is_some() || !props_is_default || !name_is_default || flags != 0;

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
        if fallback.is_some() || !props_is_default || flags != 0 {
            if !first_arg {
                create_line.push_str(", ");
            }
            create_line.push_str(props_arg.as_deref().unwrap_or("null"));
            first_arg = false;
        }
        if let Some(fallback) = fallback {
            if !first_arg {
                create_line.push_str(", ");
            }
            create_line.push_str(&fallback);
            first_arg = false;
        } else if flags != 0 {
            if !first_arg {
                create_line.push_str(", ");
            }
            create_line.push_str("null");
        }
        if flags != 0 {
            if !first_arg {
                create_line.push_str(", ");
            }
            push_u32(&mut create_line, flags);
            if !self.options.is_production {
                create_line.push_str(" /* SLOT_ROOT, INHERIT_FALLBACK */");
            }
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
            own_text_ref_generated: true,
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
        // by index (rc.5 `slots.vue`: `t0` = fallback, `t1` = root skeleton).
        //
        // Official `genTemplates` (rc.5): `root` (official `isSingleRoot`) is
        // true whenever the template has a single meaningful top-level
        // construct — for a direct root element that's `root_elements.len()
        // == 1`; for a v-if/v-else-if/v-else branch closure whose whole
        // chain IS that sole construct, the bit propagates in too
        // (`hoisted_templates`' own stored `is_root`, computed in
        // `build_closure_body` from `self.template_single_root`; rc.5
        // `elements-text/multi-root.vue` uses flag 2 only — no single root
        // there). `static` is `canUseStaticTemplate()`.
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
                    .map(|(idx, html, is_static, is_root)| {
                        (*idx, html.as_str(), *is_root, *is_static)
                    }),
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
        for root in &mut self.root_elements {
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
            // A nav-chain ref (`!own_text_ref_generated`) is already
            // established in `nav` — no extraction line.
            if let Some(text_ref) = root.own_text_ref {
                if root.own_text_ref_generated {
                    buf.push_str("  const x");
                    push_u32(&mut buf, text_ref);
                    buf.push_str(" = _txt(n");
                    push_u32(&mut buf, root.node_ref);
                    buf.push_str(")\n");
                }
            }

            // Official `flushPendingOperations` (rc.5): one-time operations
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
                    // A single effect gets a concise arrow body — matches
                    // official Vue and the sibling non-root closure path
                    // above (`build_closure_body`'s identical `len() == 1`
                    // special case). A repeated `_ctx.` read forces the
                    // braced form even here (see `repeated_reads`).
                    let hoisted = repeated_reads::hoist_repeated_ctx_reads(&mut root.effects, out);
                    buf.push_str("  _renderEffect(() => ");
                    if hoisted.is_empty() && root.effects.len() == 1 {
                        root.effects[0].write_code_into_with_anchors(&mut buf, &mut anchors);
                    } else {
                        buf.push_str("{\n");
                        for (decl_text, decl_anchors) in &hoisted {
                            buf.push_str("    ");
                            push_body_with_anchors(&mut buf, decl_text, decl_anchors, &mut anchors);
                            buf.push('\n');
                        }
                        for effect in &root.effects {
                            buf.push_str("    ");
                            effect.write_code_into_with_anchors(&mut buf, &mut anchors);
                            buf.push('\n');
                        }
                        buf.push_str("  }");
                    }
                    buf.push_str(")\n");
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
        root: &RootNodeTemplate,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        // Reset state for the template
        self.depth = 0;
        self.html.clear();
        self.html_scope_stack.clear();

        // Pre-compute whether the template has a single effective root —
        // official `hasSingleRootChild`, mirrored from
        // `vdom::VdomCodeGen::enter_template`'s `effective` count. Determines
        // `is_root` propagation into v-if/v-else branch closures below.
        let root_children = root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut effective = 0usize;
        for &child_id in root_children {
            let node = &self.ast.nodes[child_id.0];
            match &node.kind {
                AstNodeKind::Element(el) => {
                    // v-else-if / v-else continuations don't count as separate roots
                    if el.v_condition.as_ref().is_some_and(|c| {
                        matches!(
                            c.kind,
                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                        )
                    }) {
                        continue;
                    }
                    effective += 1;
                }
                AstNodeKind::Text(text) => {
                    let content = &source[text.start as usize..text.end as usize];
                    if !content.trim().is_empty() {
                        effective += 1;
                    }
                }
                AstNodeKind::Interpolation(_) => effective += 1,
                AstNodeKind::Comment(_) => {
                    if self.options.comments {
                        effective += 1;
                    }
                }
            }
        }
        self.template_single_root = effective == 1;
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
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> super::WalkAction {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );
        // Take a recycled state from the pool (retains capacity) or create new
        let mut state = self.state_pool.pop().unwrap_or_default();

        // Components, slot outlets, and template slot wrappers don't build HTML
        // templates. Neither does a `<template v-if>`/`<template v-else-if>`/
        // `<template v-else>`/`<template v-for>`: official Vue treats every
        // directive-bearing `<template>` as transparent IR (its children
        // render directly at the wrapper's position), never a literal `<template>`
        // DOM node — a bare, directive-less `<template>` stays a real element
        // (matches VDOM's identical `leave_template_fragment` gate).
        let builds_open_tag = el.tag_type != TagType::Component
            && el.tag_type != TagType::SlotOutlet
            && !(el.tag_type == TagType::Template
                && (el.v_slot.is_some() || el.v_condition.is_some() || el.v_for.is_some()));

        // A direct interpolation child only reuses this scope's own ref for
        // `_txt()` extraction when every OTHER meaningful sibling is itself
        // plain template-contributing content (text/interpolation, a rendered
        // comment, or a plain static/dynamic-attr element with no v-if/v-for).
        // A STRUCTURAL sibling (component, slot outlet, `<template v-slot>`,
        // or a v-if/v-for-wrapped element — `!is_template_contributing`,
        // reused from `has_following_template_contributing_sibling`) flips it
        // false: that sibling already reaches its position through the
        // shared `_setInsertionState`/`PendingNavRequest::Merge` nav chain,
        // and the text run must reach ITS position through that SAME chain
        // instead of a standalone `_txt()` off this scope's own ref, which
        // would silently read the WRONG DOM node once a structural sibling
        // (e.g. a `<slot>`) precedes it (official `isAllTextLike`, scoped to
        // the structural case — see `visit_interpolation`). A plain element
        // sibling with no structural construct leaves this `true`: Verter's
        // existing deferred/combined effect ordering already matches
        // official there (nothing forces an early flush), only the
        // structural case has a real flush-timing requirement.
        state.children_all_text_like = el.content.as_ref().is_none_or(|content| {
            !content.children.iter().any(|&child| {
                self.is_meaningful_sibling(child, source) && !self.is_template_contributing(child)
            })
        });

        // A component (incl. dynamic `<component :is>`/`KeepAlive`) or a
        // `<template v-slot>` wrapper has no real DOM container — read by a
        // child's merge to decide slot-root flag eligibility.
        state.is_component_scope = el.tag_type == TagType::Component
            || (el.tag_type == TagType::Template && el.v_slot.is_some());
        // A directive-bearing `<template>` (v-if/else-if/else/for, no
        // v-slot) is ALSO a no-DOM-container scope, but — confirmed against
        // the pinned oracle — it never participates in slot-root flag
        // eligibility (`is_component_scope`'s other reader), so it stays a
        // separate flag rather than folding into that one.
        state.is_transparent_wrapper = el.tag_type == TagType::Template
            && el.v_slot.is_none()
            && (el.v_condition.is_some() || el.v_for.is_some());

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
        // these two ids before any branch/item content (rc.5).
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
                let value_pattern = v_for_value_pattern(oxc);
                let map = build_for_scope_map(
                    param_part,
                    self.for_scope_depth - 1,
                    value_pattern,
                    source,
                    &self.resolver,
                    out,
                );
                self.resolver.push_for_scope(map);
            } else {
                self.resolver
                    .push_for_scope(rustc_hash::FxHashMap::default());
            }
            outer
        } else if el.tag_type == TagType::SlotOutlet {
            // Slot outlet's own construct id, same reason as v-if/v-for. Fallback
            // burns one wasted `enterBlock()` id (rc.5 `slots.vue`:
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

        // Push this v-slot scoped-slot param's destructure rename map before
        // descending — same reasoning as v-for's push above: body
        // expressions resolve during their own visit, well before
        // `leave_element`. Shares `for_scope_depth` with v-for (official's
        // `context.scopeLevel`, doc'd on the field) — a bare-identifier
        // param stays un-pushed (no scope, no depth burn), matching
        // official's `.ast`-gated `enterScope()`.
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            if let Some(leaves) = v_slot_destructure_leaves(oxc, source, &self.resolver) {
                self.for_scope_depth += 1;
                let mut base = String::with_capacity(16);
                base.push_str("_slotProps");
                helpers::push_u32(&mut base, self.for_scope_depth - 1);
                let mut map = rustc_hash::FxHashMap::default();
                for (name, accessor) in leaves {
                    accessor.register_import(out);
                    map.insert(name, accessor.render(&base));
                }
                self.resolver.push_for_scope(map);
            }
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
            && !(el.tag_type == TagType::Template
                && (el.v_slot.is_some() || el.v_condition.is_some() || el.v_for.is_some()));
        if !builds_open_tag_here || el.v_condition.is_some() || el.v_for.is_some() {
            self.block_depth -= 1;
        }
        // Pop before `build_v_for_root` so `:key` stays unrenamed (official
        // `genSimpleIdMap`) and no v-for scope leaks to siblings.
        if el.v_for.is_some() {
            self.resolver.pop_for_scope();
            self.for_scope_depth -= 1;
        }
        // Same pairing for a v-slot destructured scoped-slot param — computed
        // once here and reused below for the `_slotProps{depth}` param text
        // (`for_scope_depth` after this pop reads back this v-slot's own
        // depth, same trick `build_for_callback_params` relies on).
        let v_slot_leaves = if el.tag_type == TagType::Template && el.v_slot.is_some() {
            v_slot_destructure_leaves(oxc_el, source, &self.resolver)
        } else {
            None
        };
        if v_slot_leaves.is_some() {
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

        // Components, slot outlets, slot templates, and transparent
        // `<template v-if>`/`<template v-for>` wrappers accumulated their
        // content into the scope buffer started at `enter`; hand it to
        // `state.html` and restore the enclosing buffer before the per-kind
        // builders read it.
        if el.tag_type == TagType::Component
            || el.tag_type == TagType::SlotOutlet
            || (el.tag_type == TagType::Template
                && (el.v_slot.is_some() || el.v_condition.is_some() || el.v_for.is_some()))
        {
            self.take_scope_html(&mut state);
        }

        // Component elements
        if el.tag_type == TagType::Component {
            // Pending v-if already flushed above.
            // A statically-resolved component is a stable slot root
            // (official `markSlotRootComponent`'s `!isStatic` gate); only a
            // dynamic `<component :is>` is merge-eligible for NON_STABLE
            // forwarding.
            let merge_kind = if resolve_vapor_dynamic_component(
                el,
                tag_name,
                source,
                oxc_el,
                &self.resolver,
                self.options.force_js,
            )
            .is_some()
            {
                MergedConstructKind::DynamicComponent
            } else {
                MergedConstructKind::Other
            };
            let node_ref = state.ensure_node_ref(&mut self.counters);
            let root =
                self.build_component_root(el, tag_name, node_ref, source, state, oxc_el, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(id, root, merge_kind, source, out);
            }
            return;
        }

        // Slot outlets
        if el.tag_type == TagType::SlotOutlet {
            // Pending v-if already flushed above.
            // Reserved at enter — never mint here (fallback body would steal the id).
            let node_ref = construct_ref.expect("slot outlet always reserves a construct-own id");
            state.node_ref = Some(node_ref);
            let root = self.build_slot_outlet_root(el, oxc_el, source, node_ref, state, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(
                    id,
                    root,
                    MergedConstructKind::SlotOutlet,
                    source,
                    out,
                );
            }
            return;
        }

        // Template slot wrappers (`<template v-slot:name="params">`)
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
            // No mapped interpolation anchor in a named-slot closure today.
            let (body, _is_static, _body_anchors) =
                self.build_closure_body(state, has_dynamic_text, "    ", false, out);

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

            // Build the slot entry string: `"name": (params) => { ... }` —
            // official `genStaticSlots` always JSON-quotes a static slot
            // name (`${JSON.stringify(name)}: `), unlike a prop key's
            // needs-quoting-only rule.
            let mut entry = String::with_capacity(128);
            entry.push('"');
            helpers::escape_js_string_into(&mut entry, slot_name);
            entry.push_str("\": (");
            if v_slot_leaves.is_some() {
                // Destructured pattern renames to `_slotProps{depth}` —
                // official `genSlotBlockWithProps`'s `propsName`.
                entry.push_str("_slotProps");
                helpers::push_u32(&mut entry, self.for_scope_depth);
            } else if let Some(params) = slot_params {
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

        // Transparent `<template v-if>`/`<template v-else-if>`/`<template
        // v-else>`/`<template v-for>` (no `v-slot`): no open/close tag, no
        // attrs, no v-text — its accumulated children ARE its whole body.
        // Dispatches straight into the same v-if/v-for construct machinery a
        // normal element uses below, skipping every real-element-only step
        // (`is_void`/v-text/`close_html_tag`/`process_dynamic_props`) since
        // there is no DOM tag to close or attribute.
        if el.tag_type == TagType::Template
            && el.v_slot.is_none()
            && (el.v_condition.is_some() || el.v_for.is_some())
        {
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);

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

            let root = self.build_v_for_root(
                id,
                el,
                oxc_el,
                source,
                state,
                has_dynamic_text,
                construct_ref,
                out,
            );
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                // A v-for chain is never NON_STABLE-forwarding-eligible
                // (official `markSlotRootFor` is a distinct, unimplemented
                // branch — out of this pass's scope), same as a normal
                // element's own v-for merge below.
                self.merge_non_root_into_parent(id, root, MergedConstructKind::Other, source, out);
            }
            return;
        }

        // Normal elements
        let is_void = el.is_self_closing || el.content.is_none();

        // v-text: the element's ENTIRE DOM text content is set via
        // _setText, exactly like a lone interpolation child spanning the
        // whole element (one space placeholder in the static HTML, the
        // resolved+toDisplayString-wrapped expression in a text part).
        // Handled here — not in `process_dynamic_props` — because it needs
        // the scope HTML buffer, which that function's context doesn't
        // carry. Pushed BEFORE `close_html_tag` appends the closing tag:
        // `strip_trailing_close_tags` (Vue 3.6 minimization) trims trailing
        // whitespace before stripping a trailing `</tag>`, so a space
        // pushed AFTER the closing tag is silently swallowed along with it.
        let v_text_expr = el.props.iter().enumerate().find_map(|(idx, p)| {
            if !p.is_directive {
                return None;
            }
            let dname = &source[p.start as usize..p.name_end as usize];
            if dname != "v-text" {
                return None;
            }
            let (vs, ve) = (p.value_start?, p.value_end?);
            let value = &source[vs as usize..ve as usize];
            let oxc_exp = find_prop_oxc_exp(oxc_el, idx);
            Some(resolve_expr(
                value,
                vs,
                oxc_exp,
                &self.resolver,
                self.options.force_js,
            ))
        });
        let has_v_text = v_text_expr.is_some();
        if let Some(expr) = &v_text_expr {
            self.html.push(' ');
            let wrapped = format!("_toDisplayString({expr})");
            state.text_parts.push(super::types::VaporTextPart::Dynamic(
                out.alloc_str(&wrapped),
                &[],
            ));
            state.ensure_text_ref(&mut self.counters);
            out.add_vapor_import(VaporHelper::ToDisplayString);
        }

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

        // Derive has_dynamic_text from the AST children flags — v-text
        // counts too (it has no interpolation CHILD, but produces the same
        // dynamic-text-part/_setText shape).
        let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation) || has_v_text;

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
            let root = self.build_v_for_root(
                id,
                el,
                oxc_el,
                source,
                state,
                has_dynamic_text,
                construct_ref,
                out,
            );
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                // A v-for chain is never NON_STABLE-forwarding-eligible
                // (official `markSlotRootFor` is a distinct, unimplemented
                // branch — out of this pass's scope).
                self.merge_non_root_into_parent(id, root, MergedConstructKind::Other, source, out);
            }
            return;
        }

        // Finalize text parts into effects
        element::finalize_text_parts(&mut state, has_dynamic_text);

        // A plain element that is the SOLE meaningful content of a
        // component's implicit default slot or a `<template v-slot>`
        // wrapper (`is_component_scope`, no real DOM container) IS that
        // closure's own root — same relationship the whole template has to
        // its own depth-0 root, one level in. Its own dynamic text/effects/
        // nav become the enclosing scope's OWN fields directly; there is no
        // real DOM container between them to `_child`/`_next` into (a bare
        // `_txt(nN)` read directly off this element's own ref, not a nested
        // `_child()` hop first). A purely static such element already
        // worked (nothing to bubble), so this only changes the dynamic
        // case.
        let parent_is_sole_component_scope_content = self
            .element_stack
            .last()
            .is_some_and(|p| p.is_component_scope)
            && self.is_sole_meaningful_child(id, source);

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
        } else if parent_is_sole_component_scope_content {
            self.resolve_pending_nav_requests(&mut state, out);
            let node_ref = state.ensure_node_ref(&mut self.counters);
            if let Some(parent) = self.element_stack.last_mut() {
                parent.node_ref = Some(node_ref);
                parent.own_effects.append(&mut state.own_effects);
                parent.child_effects.append(&mut state.child_effects);
                parent.child_nav.append(&mut state.child_nav);
                parent
                    .child_text_creations
                    .append(&mut state.child_text_creations);
                parent.text_node_ref = state.text_node_ref;
                parent.text_ref_generated = state.text_ref_generated;
                parent.child_statements.append(&mut state.child_statements);
            }
            state.reset();
            self.state_pool.push(state);
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
        // Newline-containing whitespace-only text between two ELEMENT/COMMENT
        // tags is not a DOM node under Vue condense (rc.5
        // `basic-interpolation.vue` / `slots.vue` emit zero inter-tag bytes).
        // Emitting it both pollutes the skeleton and occupies a real sibling
        // that `_child`/`_next` never skip (`HierarchyRequestError: Node
        // can't be inserted in a #text parent`). When at least one neighbor
        // is text/interpolation instead (or this is leading/trailing), it
        // collapses to a single space and stays — same as a
        // WITHOUT-a-newline whitespace run (`whitespace_newline_collapses_
        // to_space`, mirroring vdom `resolve_whitespace`).
        // Reuses `vdom::text::classify_text_kind`.
        use super::vdom::text::classify_text_kind;
        let content = &source[text_node.start as usize..text_node.end as usize];
        let text_kind = classify_text_kind(content);
        if text_kind == Some(super::types::ChildKind::WhitespaceNewline)
            && !self.whitespace_newline_collapses_to_space(id)
        {
            return;
        }
        if let Some(parent) = self.element_stack.last_mut() {
            // Adjacent text/interpolation coalesce into one DOM child: advance
            // the parent's running child cursor only at the start of a run.
            if parent.observe_dom_text_run() {
                parent.text_run_html_start = Some(self.html.len());
                parent.text_run_has_dynamic = false;
            }
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
            // Once this run has an interpolation, its static text bytes must
            // NOT be baked into the hoisted HTML template — only the run's
            // single collapsed space (emitted in `visit_interpolation`)
            // stays. The text still needs its own `_setText` text part.
            let write_html = !parent.text_run_has_dynamic;
            // Reaching here with `WhitespaceNewline` means it collapsed to a
            // space (the drop case already returned above) — same handling
            // as a plain `WhitespaceSpace` run.
            if matches!(
                text_kind,
                Some(super::types::ChildKind::WhitespaceSpace)
                    | Some(super::types::ChildKind::WhitespaceNewline)
            ) {
                if write_html {
                    self.html.push(' ');
                }
                if has_interpolation {
                    // `VaporTextPart::Static` stores a JS EXPRESSION fragment
                    // (quoted string literal), not raw content — an unquoted
                    // `" "` here previously spliced as bare whitespace into
                    // the `_setText(...)` argument list (`+   +`), broken JS
                    // never reached before nothing routed a whitespace-only
                    // run through here with `has_interpolation` true.
                    parent
                        .text_parts
                        .push(super::types::VaporTextPart::Static("\" \""));
                }
            } else {
                text::process_text(
                    text_node,
                    source,
                    &mut self.html,
                    parent,
                    has_interpolation,
                    write_html,
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
            if parent.observe_dom_text_run() {
                parent.text_run_html_start = Some(self.html.len());
                parent.text_run_has_dynamic = false;
            }
            // First interpolation in this run: collapse any static text
            // already written for it down to a single space placeholder —
            // official Vue never bakes a dynamic run's static content into
            // the hoisted HTML template, only into the `_setText` runtime
            // expression. A later interpolation in the SAME run adds no
            // further space (one placeholder covers the whole run).
            if !parent.text_run_has_dynamic {
                if let Some(start) = parent.text_run_html_start {
                    self.html.truncate(start);
                }
                self.html.push(' ');
                parent.text_run_has_dynamic = true;
            }
            // Mixed-content container (`children_all_text_like == false`,
            // e.g. this text run sits next to a `<slot>`/component/v-if/
            // v-for sibling): reserve this run's nav-chain slot BEFORE
            // `process_interpolation` mints a ref — official
            // `processInterpolation`'s `context.reference()`, a fresh id
            // distinct from this scope's own container ref, resolved
            // through the same shared `_child`/`_next` chain as the
            // surrounding structural siblings instead of a standalone
            // `_txt()` extraction. Idempotent: only the run's FIRST
            // interpolation reserves a slot (`reserve_nav_text_ref` returns
            // `None` once `text_node_ref` is already set).
            if !parent.children_all_text_like {
                if let Some(text_ref) = parent.reserve_nav_text_ref(&mut self.counters) {
                    let nav_slot = parent.child_nav.len();
                    parent.child_nav.push("");
                    let stmt_slot = parent.child_statements.len();
                    parent.child_statements.push(("", &[]));
                    parent
                        .pending_nav_requests
                        .push(PendingNavRequest::TextRef {
                            own_ref: text_ref,
                            nav_slot,
                            stmt_slot,
                        });
                }
            }
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
