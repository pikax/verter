//! TSX template generation: converts Vue template AST to valid JSX.
//!
//! Walks the [`TemplateAst`] directly (not using the shared `TemplateCodeGen` trait
//! or walker) and converts Vue template syntax to JSX using `CodeTransform` mutations.
//!
//! ## Conversion rules
//!
//! | Vue syntax | JSX output |
//! |---|---|
//! | `{{ expr }}` | `{expr}` |
//! | `<!-- comment -->` | `{/* comment */}` |
//! | `:prop="expr"` | `prop={expr}` |
//! | `@event="handler"` | `onEvent={handler}` |
//! | `v-if="cond"` | `{cond ? (...) : null}` |
//! | `v-for="item in items"` | `{items.map((item) => (...))}` |
//! | `v-show="expr"` | `style={{display: expr ? undefined : 'none'}}` |
//! | `v-model="val"` | `modelValue={val} onUpdate:modelValue={...}` |
//! | `v-bind="obj"` | `{...obj}` |
//! | `v-on="obj"` | `{...obj}` |

pub mod directives;
pub mod props;

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use crate::ast::types::{
    AstNodeKind, CommentNode, ElementNode, ElementNodeConditionKind, InterpolationNode, TagType,
    TextNode,
};
use crate::ide::condition::{self, ConditionScope};
use crate::template::code_gen::binding::{BindingResolver, BindingType};
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::oxc::types::{OxcNodeData, OxcParsedAst, OxcParsedElement};
use crate::types::NodeId;

use super::IdeTemplateOptions;

/// Shared context for TSX template walker functions.
///
/// Groups the 7 parameters that are threaded identically through
/// `walk_node`, `walk_element`, and `walk_children_with_iife_tracking`.
struct IdeTemplateCtx<'a, 'alloc> {
    ast: &'a crate::ast::types::TemplateAst,
    oxc_ast: &'a OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &'a mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    resolver: &'a BindingResolver<'alloc>,
    options: &'a IdeTemplateOptions<'a>,
}

/// Generate TSX template (JSX) from the template AST.
///
/// Walks the AST and produces JSX output by overwriting Vue-specific syntax
/// with JSX equivalents. Uses `CodeGenOutput` for deferred batch operations.
pub fn generate_ide_template<'alloc>(
    ast: &crate::ast::types::TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    options: &IdeTemplateOptions<'_>,
) {
    let mut resolver = BindingResolver::new(bindings.clone(), true);
    resolver.set_tsx(true);

    let root = &ast.root;
    let content = match &root.content {
        Some(c) => c,
        None => return, // No template content
    };

    // Overwrite <template> tags
    // Replace <template> open tag with empty (we just want the content)
    out.overwrite(root.tag_open.start, root.tag_open.end, "");

    // Replace </template> close tag with empty
    if let Some(tag_close) = &root.tag_close {
        out.overwrite(tag_close.start, tag_close.end, "");
    }

    // Walk root children
    let children = &content.children;

    // Empty template: emit an empty fragment so TypeScript sees JSX usage
    if children.is_empty() {
        out.prepend_alloc(content.start, "<></>");
        return;
    }

    // If multiple root children, wrap in fragment
    let needs_fragment = children.len() > 1;
    if needs_fragment {
        out.prepend_alloc(content.start, "<>");
    }

    let mut ctx = IdeTemplateCtx {
        ast,
        oxc_ast,
        source,
        out,
        alloc,
        resolver: &resolver,
        options,
    };
    walk_children_with_iife_tracking(children, &mut ctx, &[]);

    if needs_fragment {
        ctx.out.prepend_alloc(content.end, "</>");
    }
}

/// Walk a single AST node and generate JSX output.
fn walk_node<'a, 'alloc>(
    id: NodeId,
    ctx: &mut IdeTemplateCtx<'a, 'alloc>,
    condition_scopes: &[ConditionScope],
) {
    let node = &ctx.ast.nodes[id.0];
    let oxc_data = &ctx.oxc_ast.data[id.0];

    match &node.kind {
        AstNodeKind::Element(el) => {
            let oxc_el = match oxc_data {
                OxcNodeData::Element(el) => Some(el.as_ref()),
                _ => None,
            };
            walk_element(id, el, oxc_el, ctx, condition_scopes);
        }
        AstNodeKind::Text(text) => {
            visit_text(text, ctx.source, ctx.out);
        }
        AstNodeKind::Interpolation(interp) => {
            let oxc_expr = match oxc_data {
                OxcNodeData::Interpolation(expr) => Some(expr),
                _ => None,
            };
            visit_interpolation(interp, oxc_expr, ctx.source, ctx.out, ctx.resolver);
        }
        AstNodeKind::Comment(comment) => {
            visit_comment(comment, ctx.out, ctx.options);
        }
    }
}

/// Walk an element node: handle directives, props, children.
fn walk_element<'a, 'alloc>(
    id: NodeId,
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    ctx: &mut IdeTemplateCtx<'a, 'alloc>,
    parent_condition_scopes: &[ConditionScope],
) {
    // Handle structural directives first
    let has_v_if = el.v_condition.is_some();
    let has_v_for = el.v_for.is_some();
    // <template v-if v-slot> — v-if is handled by slot codegen, skip IIFE wrapping
    let is_slot_template = el.tag_type == TagType::Template && has_v_if && el.v_slot.is_some();
    // v-if + v-for on same element: use ternary instead of IIFE (IIFE is invalid inside v-for's
    // parenthesized expression body — it's parsed as an object literal)
    let has_v_if_with_v_for = has_v_for
        && el
            .v_condition
            .as_ref()
            .is_some_and(|c| c.kind == ElementNodeConditionKind::If);
    let emit_iife = has_v_if && !is_slot_template && !has_v_if_with_v_for;

    // v-for wrapping
    if has_v_for {
        directives::emit_v_for_open(el, oxc_el, ctx.source, ctx.out, ctx.alloc, ctx.resolver);
    }

    // v-if + v-for ternary: emitted after v-for open so the ternary is inside the map body
    if has_v_if_with_v_for {
        directives::emit_v_if_ternary_open(el, oxc_el, ctx.source, ctx.out, ctx.resolver);
    }

    // Build condition scope for this element (for type narrowing guards).
    // This computes the current element's scope and the full accumulated scopes.
    let own_scope = if has_v_if {
        build_condition_scope(el, oxc_el, ctx.source, ctx.resolver, ctx.ast, id)
    } else {
        None
    };
    let full_scopes: Vec<ConditionScope> = if let Some(ref scope) = own_scope {
        let mut s = parent_condition_scopes.to_vec();
        s.push(scope.clone());
        s
    } else {
        parent_condition_scopes.to_vec()
    };

    // Generate guard text for prop narrowing (full accumulated scopes)
    let guard_text = condition::generate_condition_text(&full_scopes);

    // v-if/v-else-if/v-else IIFE wrapping (skip for <template v-if v-slot>)
    if emit_iife {
        directives::emit_v_if_open(
            el,
            oxc_el,
            ctx.source,
            ctx.out,
            ctx.alloc,
            ctx.resolver,
            parent_condition_scopes,
        );
    }

    // Remove cached structural directive attributes from source.
    // These are NOT in el.props (the parser extracts them via prop.take()),
    // but their byte ranges are still in the original source. Without explicit
    // removal they leak into the JSX output as invalid attributes.
    // We also consume leading whitespace so `<div v-once>` → `<div>`, not `<div >`.
    if let Some(ref condition) = el.v_condition {
        let start = eat_leading_whitespace(ctx.source, condition.prop.start);
        let prop_end = props::get_prop_end(&condition.prop);
        ctx.out.overwrite(start, prop_end, "");
    }
    if let Some(ref v_for) = el.v_for {
        let start = eat_leading_whitespace(ctx.source, v_for.start);
        let prop_end = props::get_prop_end(v_for);
        ctx.out.overwrite(start, prop_end, "");
    }
    // Skip v_slot removal for <template> — handled in the TagType::Template branch
    // which preserves the slot name at its original position for sourcemaps.
    if el.tag_type != TagType::Template {
        if let Some(ref v_slot) = el.v_slot {
            let start = eat_leading_whitespace(ctx.source, v_slot.start);
            let prop_end = props::get_prop_end(v_slot);
            ctx.out.overwrite(start, prop_end, "");
        }
    }
    if let Some(ref v_once) = el.v_once {
        let start = eat_leading_whitespace(ctx.source, v_once.start);
        let prop_end = props::get_prop_end(v_once);
        ctx.out.overwrite(start, prop_end, "");
    }
    // Convert cached `ref` attribute to JSX expression syntax.
    // `ref="myRef"` → `ref={"myRef"}` (static string ref)
    // `:ref="expr"` → `ref={expr}` (dynamic binding ref)
    if let Some(ref v_ref) = el.v_ref {
        if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
            let prop_end = props::get_prop_end(v_ref);
            if v_ref.is_directive {
                // :ref="expr" or v-bind:ref="expr" → ref={expr}
                let value = &ctx.source[vs as usize..ve as usize];
                ctx.out
                    .overwrite(v_ref.start, prop_end, &format!("ref={{{}}}", value));
            } else {
                // ref="myRef" → ref={"myRef"}
                let value = &ctx.source[vs as usize..ve as usize];
                ctx.out
                    .overwrite(v_ref.start, prop_end, &format!("ref={{\"{}\"}}", value));
            }
        }
    }

    // Handle the element tag itself
    let tag_name = &ctx.source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

    // Convert tag for components
    match el.tag_type {
        TagType::Component => {
            // Handle `<component is="...">` / `<component :is="...">`.
            if tag_name == "component" {
                rewrite_component_is(el, oxc_el, ctx.source, ctx.out, ctx.resolver);
            }
        }
        TagType::Template => {
            // Named slot: preserve slot name at original position for intellisense.
            // <template #header> → <>{"header"}
            // <template v-slot:header> → <>{"header"}
            if let Some(ref v_slot) = el.v_slot {
                if let (Some(arg_start), Some(arg_end)) = (v_slot.arg_start, v_slot.arg_end) {
                    // Overwrite everything before slot name → <>{"
                    ctx.out.overwrite(el.tag_open.start, arg_start, "<>{\"");
                    // Slot name stays at [arg_start, arg_end) — sourcemap preserves position
                    // Overwrite everything after slot name through close of open tag → "}
                    ctx.out.overwrite(arg_end, el.tag_open.end, "\"}");
                } else {
                    // Default slot (no name): <template v-slot> → <>
                    ctx.out.overwrite(el.tag_open.start, el.tag_open.end, "<>");
                }
            } else {
                // Plain <template> wrapper → <>
                ctx.out.overwrite(el.tag_open.start, el.tag_open.end, "<>");
            }
        }
        TagType::SlotOutlet => {
            // <slot name="x" :prop="val">fallback</slot>
            // → {$slots.x?.({ prop: val }) ?? <>fallback</>}
            //
            // For TSX type checking, emit the slot call so TypeScript can
            // verify slot props and return types.
            let has_children = el.content.as_ref().is_some_and(|c| !c.children.is_empty());

            // Extract slot name from props (static `name` or `:name`)
            let slot_name = extract_slot_name(el, ctx.source).unwrap_or("default");

            // Collect slot props (non-name, non-structural attributes)
            let slot_props = collect_slot_props(el, oxc_el, ctx.source, ctx.resolver);

            // Build the call expression
            let call = if slot_props.is_empty() {
                format!("___VERTER___instance.$slots.{}?.()", slot_name)
            } else {
                format!(
                    "___VERTER___instance.$slots.{}?.({{ {} }})",
                    slot_name, slot_props
                )
            };

            // Overwrite open tag with slot call prefix
            if has_children {
                ctx.out.overwrite(
                    el.tag_open.start,
                    el.tag_open.end,
                    &format!("{{{} ?? <>", call),
                );
            } else {
                ctx.out
                    .overwrite(el.tag_open.start, el.tag_open.end, &format!("{{{}}}", call));
            }

            // Close tag
            if let Some(tag_close) = &el.tag_close {
                if has_children {
                    ctx.out.overwrite(tag_close.start, tag_close.end, "</>}");
                } else {
                    ctx.out.overwrite(tag_close.start, tag_close.end, "");
                }
            }

            // Skip normal prop processing and child walking for slot outlets —
            // we've already handled everything above. Process children below
            // only if there's fallback content.
            if has_children {
                if let Some(content) = &el.content {
                    walk_children_with_iife_tracking(&content.children, ctx, &full_scopes);
                }
            }

            // Close v-if/v-for if present
            if emit_iife {
                directives::emit_v_if_close(el, ctx.source, ctx.out);
            }
            if has_v_if_with_v_for {
                directives::emit_v_if_ternary_close(el, ctx.out);
            }
            if has_v_for {
                directives::emit_v_for_close(el, ctx.source, ctx.out);
            }
            return; // Early return — skip normal element processing below
        }
        _ => {
            // Native HTML elements — pass through
        }
    }

    // Process props/attributes → JSX (pass guard for type narrowing in arrow functions)
    props::process_element_props(
        el,
        oxc_el,
        ctx.source,
        ctx.out,
        ctx.alloc,
        ctx.resolver,
        guard_text.as_deref(),
        ctx.options.is_jsx,
    );

    // Process v-show
    directives::emit_v_show(el, oxc_el, ctx.source, ctx.out, ctx.alloc, ctx.resolver);

    // Walk children — children inherit the condition scopes from this element
    if let Some(content) = &el.content {
        walk_children_with_iife_tracking(&content.children, ctx, &full_scopes);
    }

    // Handle close tag for <template> → </>
    if el.tag_type == TagType::Template {
        if let Some(tag_close) = &el.tag_close {
            ctx.out.overwrite(tag_close.start, tag_close.end, "</>");
        }
    }

    // Close v-if IIFE (skip for <template v-if v-slot>)
    if emit_iife {
        directives::emit_v_if_close(el, ctx.source, ctx.out);
    }

    // Close v-if ternary before v-for close
    if has_v_if_with_v_for {
        directives::emit_v_if_ternary_close(el, ctx.out);
    }

    // Close v-for
    if has_v_for {
        directives::emit_v_for_close(el, ctx.source, ctx.out);
    }
}

/// Scan forward from `start_idx + 1`, skipping whitespace-only text nodes and comments,
/// and return `true` if the next non-whitespace, non-comment child is a v-else-if or v-else element.
/// This prevents premature IIFE closure when formatted templates have whitespace/comments
/// between v-if and v-else-if/v-else elements.
fn next_sibling_continues_v_if_chain(
    children: &[NodeId],
    start_idx: usize,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> bool {
    for &sibling_id in &children[start_idx + 1..] {
        let sibling_node = &ast.nodes[sibling_id.0];
        match &sibling_node.kind {
            AstNodeKind::Text(t) => {
                let text = &source[t.start as usize..t.end as usize];
                if text.trim().is_empty() {
                    continue; // skip whitespace-only text
                }
                return false; // non-whitespace text → chain broken
            }
            AstNodeKind::Comment(_) => {
                continue; // skip comments
            }
            AstNodeKind::Element(el) => {
                if let Some(ref cond) = el.v_condition {
                    return matches!(
                        cond.kind,
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                    );
                }
                return false; // non-conditional element → chain broken
            }
            _ => return false,
        }
    }
    false
}

/// Walk a list of child nodes, tracking pending IIFE closures for v-if chains.
///
/// When a v-if or v-else-if element is encountered, the IIFE block is left open
/// (only the if/else-if block is closed by `emit_v_if_close`). The parent loop
/// must close the IIFE (`}}`) when:
/// - A new v-if chain starts (flush previous pending)
/// - A non-conditional element follows (flush pending)
/// - The end of the children list is reached (flush pending)
///
/// v-else elements close the entire IIFE themselves (`}}}`), so no pending close
/// is needed after them.
fn walk_children_with_iife_tracking<'a, 'alloc>(
    children: &[NodeId],
    ctx: &mut IdeTemplateCtx<'a, 'alloc>,
    parent_condition_scopes: &[ConditionScope],
) {
    let mut pending_iife_close_pos: Option<u32> = None;

    // Pre-scan: identify comment children that immediately precede v-if elements.
    // These will be removed from their original position and re-emitted inside the IIFE
    // so that @ts-expect-error / @ts-ignore directives apply to the conditional content.
    let comment_reposition_set = if ctx.options.comments {
        find_comments_before_v_if(children, ctx.ast, ctx.source)
    } else {
        rustc_hash::FxHashSet::default()
    };

    for (idx, &child_id) in children.iter().enumerate() {
        let child_node = &ctx.ast.nodes[child_id.0];

        // Skip comments that will be repositioned inside IIFE
        if comment_reposition_set.contains(&idx) {
            if let AstNodeKind::Comment(c) = &child_node.kind {
                ctx.out.overwrite(c.start, c.end, "");
            }
            continue;
        }

        // Check if this child needs us to flush a pending IIFE close.
        // Elements with v-if + v-for use ternary instead of IIFE — treat them
        // like non-conditional elements for IIFE tracking purposes.
        if let AstNodeKind::Element(child_el) = &child_node.kind {
            let uses_ternary = child_el.v_for.is_some()
                && child_el
                    .v_condition
                    .as_ref()
                    .is_some_and(|c| c.kind == ElementNodeConditionKind::If);
            if !uses_ternary {
                if let Some(ref cond) = child_el.v_condition {
                    match cond.kind {
                        ElementNodeConditionKind::If => {
                            // New chain — flush pending close from any previous chain
                            if let Some(pos) = pending_iife_close_pos.take() {
                                ctx.out.prepend_alloc(pos, "}}");
                            }
                        }
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
                            // Continuation of existing chain — don't flush
                        }
                    }
                } else {
                    // Non-conditional element — flush pending
                    if let Some(pos) = pending_iife_close_pos.take() {
                        ctx.out.prepend_alloc(pos, "}}");
                    }
                }
            } else {
                // v-if + v-for uses ternary — flush pending
                if let Some(pos) = pending_iife_close_pos.take() {
                    ctx.out.prepend_alloc(pos, "}}");
                }
            }
        } else {
            // Text/comment/interpolation — flush pending, unless the next
            // non-whitespace sibling continues the v-if chain (v-else-if/v-else)
            if pending_iife_close_pos.is_some()
                && !next_sibling_continues_v_if_chain(children, idx, ctx.ast, ctx.source)
            {
                if let Some(pos) = pending_iife_close_pos.take() {
                    ctx.out.prepend_alloc(pos, "}}");
                }
            }
        }

        walk_node(child_id, ctx, parent_condition_scopes);

        // After walking a v-if element, inject repositioned comments inside the IIFE
        if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
            if matches!(
                child_el.v_condition.as_ref().map(|c| &c.kind),
                Some(ElementNodeConditionKind::If)
            ) && !comment_reposition_set.is_empty()
            {
                inject_repositioned_comments(
                    idx,
                    children,
                    &comment_reposition_set,
                    ctx.ast,
                    ctx.source,
                    ctx.out,
                    child_el,
                    ctx.alloc,
                );
            }
        }

        // After walking, track IIFE close position for v-if/v-else-if.
        // Skip for v-if + v-for elements which use ternary instead of IIFE.
        if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
            let uses_ternary = child_el.v_for.is_some()
                && child_el
                    .v_condition
                    .as_ref()
                    .is_some_and(|c| c.kind == ElementNodeConditionKind::If);
            if !uses_ternary {
                if let Some(ref cond) = child_el.v_condition {
                    let el_end = child_el
                        .tag_close
                        .as_ref()
                        .map(|tc| tc.end)
                        .unwrap_or(child_el.tag_open.end);
                    match cond.kind {
                        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                            pending_iife_close_pos = Some(el_end);
                        }
                        ElementNodeConditionKind::Else => {
                            // v-else already closed with }}} — no pending needed
                            pending_iife_close_pos = None;
                        }
                    }
                }
            }
        }
    }

    // After all children: flush any remaining pending IIFE close
    if let Some(pos) = pending_iife_close_pos.take() {
        ctx.out.prepend_alloc(pos, "}}");
    }
}

/// Pre-scan children to find comment indices that immediately precede v-if elements.
/// Returns a set of child indices whose comments should be repositioned inside the IIFE.
/// Only consecutive comments (with optional whitespace text between) are collected;
/// any non-comment/non-whitespace node resets the collection.
fn find_comments_before_v_if(
    children: &[NodeId],
    ast: &crate::ast::types::TemplateAst,
    source: &str,
) -> rustc_hash::FxHashSet<usize> {
    let mut set = rustc_hash::FxHashSet::default();
    for (i, &child_id) in children.iter().enumerate() {
        let node = &ast.nodes[child_id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            if matches!(
                el.v_condition.as_ref().map(|c| &c.kind),
                Some(ElementNodeConditionKind::If)
            ) {
                // Walk backward to find consecutive comments
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    let prev = &ast.nodes[children[j].0];
                    match &prev.kind {
                        AstNodeKind::Comment(_) => {
                            set.insert(j);
                        }
                        AstNodeKind::Text(t) => {
                            let text = &source[t.start as usize..t.end as usize];
                            if text.trim().is_empty() {
                                continue; // Skip whitespace-only text
                            }
                            break; // Non-whitespace text — stop
                        }
                        _ => break,
                    }
                }
            }
        }
    }
    set
}

/// After walking a v-if element, emit repositioned comments inside the IIFE.
/// Comments are emitted in forward order at the element's tag_open position,
/// which places them after the IIFE open (`{()=>{if(cond){\n`} already prepended there.
#[allow(clippy::too_many_arguments)]
fn inject_repositioned_comments<'alloc>(
    v_if_idx: usize,
    children: &[NodeId],
    reposition_set: &rustc_hash::FxHashSet<usize>,
    ast: &crate::ast::types::TemplateAst,
    source: &str,
    out: &mut CodeGenOutput<'alloc>,
    el: &ElementNode,
    _alloc: &'alloc Allocator,
) {
    // Find the first repositioned comment before this v-if element
    let mut first_comment = v_if_idx;
    for j in (0..v_if_idx).rev() {
        if reposition_set.contains(&j) {
            first_comment = j;
        } else {
            let prev = &ast.nodes[children[j].0];
            if let AstNodeKind::Text(t) = &prev.kind {
                if source[t.start as usize..t.end as usize].trim().is_empty() {
                    continue; // Skip whitespace text between comments
                }
            }
            break;
        }
    }

    // Emit comments in forward order.
    // Use mapped_prepend (with offset = content.len() → effectively unmapped) so that
    // these comments stay in the mapped_prepends vec and maintain correct insertion order
    // relative to the IIFE opening which is also emitted via mapped_prepends.
    for (j, &child_id) in children
        .iter()
        .enumerate()
        .take(v_if_idx)
        .skip(first_comment)
    {
        if !reposition_set.contains(&j) {
            continue;
        }
        let prev = &ast.nodes[child_id.0];
        if let AstNodeKind::Comment(c) = &prev.kind {
            let text = &source[c.content_start as usize..c.content_end as usize];
            let jsx_comment = format!("{{/*{}*/}}\n", text);
            let len = jsx_comment.len() as u32;
            out.prepend_alloc_mapped_with_offset(el.tag_open.start, 0, len, &jsx_comment);
        }
    }
}

/// Build a [`ConditionScope`] for a v-if/v-else-if/v-else element.
///
/// Walks backward through siblings to collect sibling negation conditions,
/// and resolves the element's own condition with binding prefixes.
fn build_condition_scope<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &str,
    resolver: &BindingResolver<'alloc>,
    ast: &crate::ast::types::TemplateAst,
    node_id: NodeId,
) -> Option<ConditionScope> {
    let condition = el.v_condition.as_ref()?;

    // Resolve own condition expression (positive)
    let positive = match condition.kind {
        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
            let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end)
            else {
                return None;
            };
            let raw_expr = &source[vs as usize..ve as usize];
            Some(directives::resolve_condition_expr_pub(
                raw_expr, vs, oxc_el, resolver,
            ))
        }
        ElementNodeConditionKind::Else => None,
    };

    // Collect sibling negations by walking backward
    let sibling_negations = match condition.kind {
        ElementNodeConditionKind::If => vec![],
        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
            collect_sibling_negations(ast, node_id, source, resolver)
        }
    };

    Some(ConditionScope {
        positive,
        sibling_negations,
    })
}

/// Walk backward through siblings of a v-else-if/v-else element to collect
/// the resolved condition expressions of preceding v-if and v-else-if elements.
fn collect_sibling_negations<'alloc>(
    ast: &crate::ast::types::TemplateAst,
    node_id: NodeId,
    source: &str,
    resolver: &BindingResolver<'alloc>,
) -> Vec<String> {
    let mut negations = Vec::new();
    let mut current = node_id;

    while let Some(prev) = ast.prev_sibling(current) {
        let prev_node = &ast.nodes[prev.0];
        match &prev_node.kind {
            AstNodeKind::Element(prev_el) => {
                if let Some(ref cond) = prev_el.v_condition {
                    // Resolve the sibling's condition expression
                    if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                        let raw_expr = &source[vs as usize..ve as usize];
                        let resolved = resolver.resolve_simple_expr(raw_expr);
                        negations.push(resolved);
                    }

                    // If we hit a v-if, that's the start of the chain — stop
                    if matches!(cond.kind, ElementNodeConditionKind::If) {
                        break;
                    }
                } else {
                    // Non-conditional element — stop (not part of the chain)
                    break;
                }
            }
            AstNodeKind::Text(text) => {
                // Skip whitespace-only text nodes
                let t = &source[text.start as usize..text.end as usize];
                if t.trim().is_empty() {
                    current = prev;
                    continue;
                }
                break; // Non-whitespace text — stop
            }
            AstNodeKind::Comment(_) => {
                // Skip comments
                current = prev;
                continue;
            }
            _ => break,
        }
        current = prev;
    }

    // Reverse so they're in chain order (v-if first, then v-else-if's)
    negations.reverse();
    negations
}

fn rewrite_component_is<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    let static_is_prop = el.props.iter().find(|prop| {
        if prop.is_directive {
            return false;
        }
        &source[prop.start as usize..prop.name_end as usize] == "is"
    });

    // 1) Static `is="div"`
    if let Some(is_prop) = static_is_prop {
        let (Some(value_start), Some(value_end)) = (is_prop.value_start, is_prop.value_end) else {
            return;
        };
        if value_end <= value_start {
            return;
        }

        let target_tag = source[value_start as usize..value_end as usize].trim();
        if target_tag.is_empty() {
            return;
        }

        rewrite_component_tag_name(el, target_tag, out);

        // Remove `is="..."`
        let is_prop_end = props::get_prop_end(is_prop);
        out.overwrite(is_prop.start, is_prop_end, "");
        return;
    }

    // 2) Dynamic `:is="expr"` / `v-bind:is="expr"`
    let bind_is_result = el.props.iter().enumerate().find(|(_, prop)| {
        if !prop.is_directive || prop.is_dynamic == Some(true) {
            return false;
        }
        if directive_name(prop, source) != "bind" {
            return false;
        }
        let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) else {
            return false;
        };
        source[arg_start as usize..arg_end as usize].trim() == "is"
    });

    let Some((bind_is_index, bind_is_prop)) = bind_is_result else {
        return;
    };

    let (Some(value_start), Some(value_end)) = (bind_is_prop.value_start, bind_is_prop.value_end)
    else {
        return;
    };
    if value_end <= value_start {
        return;
    }

    let value_expr = source[value_start as usize..value_end as usize].trim();
    if value_expr.is_empty() {
        return;
    }

    // All dynamic :is expressions use extractRenderComponent wrapper.
    // Resolve binding prefixes (e.g., _ctx. for Data bindings)
    let oxc_prop = oxc_el.and_then(|el| el.props.iter().find(|p| p.prop_index == bind_is_index));
    let resolved_expr = if let Some(oxc_p) = oxc_prop {
        if let Some(ref exp) = oxc_p.exp {
            use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
            build_prefixed_expr(value_expr, value_start, exp, resolver, &[])
        } else {
            resolver.resolve_simple_expr(value_expr)
        }
    } else {
        resolver.resolve_simple_expr(value_expr)
    };

    let temp_name = "___VERTER___component_render";
    let prefix = format!("const {}=___VERTER___extractRenderComponent(", temp_name);
    let content = format!("{}{});\n", prefix, resolved_expr);
    // Use mapped emission so the expression gets a source map token.
    // This allows TSGO to map hover positions back to the Vue template.
    out.prepend_alloc_mapped_with_offset(
        el.tag_open.start,
        value_start,
        prefix.len() as u32,
        &content,
    );
    rewrite_component_tag_name(el, temp_name, out);

    // Remove `:is="..."`
    let prop_end = props::get_prop_end(bind_is_prop);
    out.overwrite(bind_is_prop.start, prop_end, "");
}

fn rewrite_component_tag_name(el: &ElementNode, target_tag: &str, out: &mut CodeGenOutput<'_>) {
    // Rewrite opening `<component` to `<targetTag`.
    out.overwrite(el.tag_open.start + 1, el.tag_open.name_end, target_tag);

    // Rewrite closing `</component>` if present.
    if let Some(tag_close) = &el.tag_close {
        out.overwrite(tag_close.start + 2, tag_close.name_end, target_tag);
    }
}

/// Walk backwards from `pos` to consume preceding ASCII whitespace (spaces/tabs).
/// Returns the earliest position that is still whitespace, so the overwrite range
/// `[eat_leading_whitespace(source, prop.start) .. prop_end]` removes the attribute
/// AND the space before it (e.g., `<div v-once>` → `<div>`).
fn eat_leading_whitespace(source: &str, pos: u32) -> u32 {
    let bytes = source.as_bytes();
    let mut i = pos as usize;
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
        i -= 1;
    }
    i as u32
}

fn directive_name<'a>(prop: &crate::types::NodeProp, source: &'a str) -> &'a str {
    let name = &source[prop.start as usize..prop.name_end as usize];
    if name.starts_with(':') || name.starts_with('.') {
        return "bind";
    }
    if name.starts_with('@') {
        return "on";
    }
    if name.starts_with('#') {
        return "slot";
    }
    name.strip_prefix("v-").unwrap_or(name)
}

/// Extract the slot name from a `<slot>` element's attributes.
///
/// - `<slot>` → `None` (will use "default")
/// - `<slot name="header">` → `Some("header")`
/// - `<slot :name="dynamicName">` → `None` (dynamic, falls back to "default")
fn extract_slot_name<'a>(el: &ElementNode, source: &'a str) -> Option<&'a str> {
    for prop in &el.props {
        if !prop.is_directive {
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let name = source[vs as usize..ve as usize].trim();
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
        } else {
            let dir_name = directive_name(prop, source);
            if dir_name == "bind" {
                if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[arg_s as usize..arg_e as usize];
                    if arg == "name" {
                        // Dynamic :name — can't resolve statically
                        return None;
                    }
                }
            }
        }
    }
    None
}

/// Collect slot outlet props as a comma-separated string of `key: value` pairs.
///
/// Excludes the `name` attribute (used for slot identification, not passed as props).
fn collect_slot_props(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> String {
    use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;

    let mut parts = Vec::new();

    for (i, prop) in el.props.iter().enumerate() {
        if !prop.is_directive {
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                continue; // Skip name attribute
            }
            // Static attribute: name="value"
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                parts.push(format!("{}: \"{}\"", attr_name, value));
            } else {
                parts.push(format!("{}: true", attr_name));
            }
        } else {
            let dir_name = directive_name(prop, source);
            match dir_name {
                "bind" => {
                    if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                        let arg = &source[arg_s as usize..arg_e as usize];
                        if arg == "name" {
                            continue; // Skip :name
                        }
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let raw = &source[vs as usize..ve as usize];
                            let oxc_prop =
                                oxc_el.and_then(|e| e.props.iter().find(|p| p.prop_index == i));
                            let resolved = if let Some(oxc_p) = oxc_prop {
                                if let Some(ref exp) = oxc_p.exp {
                                    build_prefixed_expr(raw, vs, exp, resolver, &[])
                                } else {
                                    resolver.resolve_simple_expr(raw)
                                }
                            } else {
                                resolver.resolve_simple_expr(raw)
                            };
                            parts.push(format!("{}: {}", arg, resolved));
                        }
                    }
                }
                _ => {
                    // Skip other directives on slot outlets (v-if etc handled separately)
                }
            }
        }
    }

    parts.join(", ")
}

/// Visit a text node.
///
/// To keep TSX valid for content like `2 < 1`, non-empty trimmed text is wrapped
/// as a string expression (`{"..."}`), matching v5/process text-plugin semantics.
/// Whitespace-only text remains unchanged.
fn visit_text(text: &TextNode, source: &str, out: &mut CodeGenOutput<'_>) {
    if text.end <= text.start {
        return;
    }

    let raw_text = &source[text.start as usize..text.end as usize];
    let trimmed = raw_text.trim();
    if trimmed.is_empty() || trimmed == "<" {
        return;
    }

    let Some(rel_start) = raw_text.find(trimmed) else {
        return;
    };

    let content_start = text.start + rel_start as u32;
    let content_end = content_start + trimmed.len() as u32;

    // Escape quotes inside the string literal body.
    for (i, b) in trimmed.as_bytes().iter().enumerate() {
        if *b == b'"' {
            let pos = content_start + i as u32;
            out.overwrite(pos, pos + 1, "\\\"");
        }
    }

    out.prepend_alloc(content_start, "{\"");
    out.prepend_alloc(content_end, "\"}");
}

/// Visit an interpolation node: `{{ expr }}` → `{expr}`.
fn visit_interpolation<'alloc>(
    interp: &InterpolationNode,
    oxc_expr: Option<&crate::template::oxc::types::OxcParsedExpression<'alloc>>,
    _source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    // Replace `{{` with `{`
    out.overwrite(interp.start, interp.inner_start, "{");

    // Apply binding prefixes to expression identifiers
    if let Some(expr) = oxc_expr {
        if let Some(ref bindings) = expr.bindings {
            resolver.collect_binding_patches(bindings, out);
        }
    }

    // Replace `}}` with `}`
    out.overwrite(interp.inner_end, interp.end, "}");
}

/// Visit a comment node: `<!-- text -->` → `{/* text */}`.
fn visit_comment(
    comment: &CommentNode,
    out: &mut CodeGenOutput<'_>,
    options: &IdeTemplateOptions<'_>,
) {
    if !options.comments {
        // Strip comment entirely
        out.overwrite(comment.start, comment.end, "");
        return;
    }

    // Convert HTML comment to JSX comment
    // <!-- → {/*  and  --> → */}
    // Keep original comment-inner spacing untouched.
    out.overwrite(comment.start, comment.content_start, "{/*");
    out.overwrite(comment.content_end, comment.end, "*/}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_transform::CodeTransform;

    /// Helper: compile a full SFC with TSX template generation.
    /// Returns the template portion of the TSX output.
    fn gen_tsx_template(source: &str) -> String {
        let alloc = Allocator::new();
        let bytes = source.as_bytes();

        // Parse SFC
        let mut syntax = crate::parser::Syntax::new(false);
        crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
            syntax.handle(
                &e,
                &crate::diagnostics::SyntaxPluginContext {
                    input: source,
                    bytes,
                    options: &crate::diagnostics::SyntaxPluginOptions::default(),
                    diagnostics: Vec::new(),
                },
            )
        });

        let template_ast = match syntax.take_template_ast() {
            Some(ast) => ast,
            None => return String::new(),
        };

        // Parse template expressions
        let source_type = oxc_span::SourceType::tsx();
        let oxc_ast = crate::template::oxc::parse_template_expressions(
            &template_ast,
            source,
            &alloc,
            source_type,
        );

        // Generate TSX template
        let tpl_alloc = Allocator::new();
        let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
        let mut out = CodeGenOutput::new(&tpl_alloc);
        let bindings = FxHashMap::default();
        let options = IdeTemplateOptions {
            self_name: "App",
            comments: true,
            is_jsx: false,
        };

        generate_ide_template(
            &template_ast,
            &oxc_ast,
            source,
            &mut out,
            &tpl_alloc,
            &bindings,
            &options,
        );
        out.apply_to(&mut tpl_ct);

        let full = tpl_ct.build_string();

        // Extract just the template region
        let tpl_start = template_ast.root.tag_open.start as usize;
        let tpl_end = template_ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end as usize)
            .unwrap_or(full.len());
        let suffix_len = source.len() - tpl_end;
        full[tpl_start..full.len() - suffix_len].to_string()
    }

    fn gen_tsx_template_with_bindings(source: &str, bindings: &[(&str, BindingType)]) -> String {
        let alloc = Allocator::new();
        let bytes = source.as_bytes();

        let mut syntax = crate::parser::Syntax::new(false);
        crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
            syntax.handle(
                &e,
                &crate::diagnostics::SyntaxPluginContext {
                    input: source,
                    bytes,
                    options: &crate::diagnostics::SyntaxPluginOptions::default(),
                    diagnostics: Vec::new(),
                },
            )
        });

        let template_ast = match syntax.take_template_ast() {
            Some(ast) => ast,
            None => return String::new(),
        };

        let source_type = oxc_span::SourceType::tsx();
        let oxc_ast = crate::template::oxc::parse_template_expressions(
            &template_ast,
            source,
            &alloc,
            source_type,
        );

        let tpl_alloc = Allocator::new();
        let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
        let mut out = CodeGenOutput::new(&tpl_alloc);

        let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
        for &(name, bt) in bindings {
            binding_map.insert(tpl_alloc.alloc_str(name), bt);
        }

        let options = IdeTemplateOptions {
            self_name: "App",
            comments: true,
            is_jsx: false,
        };

        generate_ide_template(
            &template_ast,
            &oxc_ast,
            source,
            &mut out,
            &tpl_alloc,
            &binding_map,
            &options,
        );
        out.apply_to(&mut tpl_ct);

        let full = tpl_ct.build_string();
        let tpl_start = template_ast.root.tag_open.start as usize;
        let tpl_end = template_ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end as usize)
            .unwrap_or(full.len());
        let suffix_len = source.len() - tpl_end;
        full[tpl_start..full.len() - suffix_len].to_string()
    }

    // ── Basic nodes ────────────────────────────────────────────

    #[test]
    fn basic_div() {
        let result = gen_tsx_template("<template><div></div></template>");
        assert!(result.contains("<div></div>"), "got: {}", result);
    }

    #[test]
    fn text_content() {
        let result = gen_tsx_template("<template><div>hello</div></template>");
        assert!(result.contains("<div>{\"hello\"}</div>"), "got: {}", result);
    }

    #[test]
    fn text_content_with_lt_wrapped() {
        let result = gen_tsx_template("<template>2 < 1</template>");
        assert!(
            result.contains("{\"2 < 1\"}")
                || (result.contains("{\"2\"}") && result.contains("{\"< 1\"}")),
            "got: {}",
            result
        );
    }

    #[test]
    fn text_content_escapes_quote() {
        let result = gen_tsx_template("<template>\"</template>");
        assert!(result.contains("{\"\\\"\"}"), "got: {}", result);
    }

    #[test]
    fn interpolation_basic() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ msg }}</div></template>",
            &[("msg", BindingType::SetupRef)],
        );
        assert!(
            result.contains("{ msg }"),
            "{{ msg }} should become bare identifier in TSX mode, got: {}",
            result
        );
    }

    #[test]
    fn interpolation_expression() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ a + b }}</div></template>",
            &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
        );
        assert!(result.contains("{ a + b }"), "got: {}", result);
    }

    #[test]
    fn comment_preserved() {
        let result = gen_tsx_template("<template><!-- hello --></template>");
        assert!(
            result.contains("{/* hello */}"),
            "Comment should be converted to JSX, got: {}",
            result
        );
    }

    #[test]
    fn self_closing_element() {
        let result = gen_tsx_template("<template><br/></template>");
        assert!(result.contains("<br/>"), "got: {}", result);
    }

    #[test]
    fn nested_elements() {
        let result = gen_tsx_template("<template><div><span></span></div></template>");
        assert!(
            result.contains("<div><span></span></div>"),
            "got: {}",
            result
        );
    }

    #[test]
    fn multiple_root_elements() {
        let result = gen_tsx_template("<template><div></div><span></span></template>");
        assert!(
            result.contains("<>") && result.contains("</>"),
            "Multiple root elements should be wrapped in fragment, got: {}",
            result
        );
    }

    // ── Interpolation with bindings ────────────────────────────

    #[test]
    fn interpolation_with_setup_ref() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ count }}</div></template>",
            &[("count", BindingType::SetupRef)],
        );
        // In TSX mode, SetupRef gets no prefix and no .value suffix (block scope handles unwrapping)
        assert!(
            result.contains("{ count }") && !result.contains("count.value"),
            "SetupRef should be bare identifier in TSX mode (no .value), got: {}",
            result
        );
    }

    #[test]
    fn interpolation_with_setup_const() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ msg }}</div></template>",
            &[("msg", BindingType::SetupConst)],
        );
        // SetupConst in inline mode: no prefix, no suffix
        assert!(
            result.contains("{ msg }"),
            "SetupConst should have no prefix/suffix, got: {}",
            result
        );
    }

    #[test]
    fn interpolation_with_props() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ title }}</div></template>",
            &[("title", BindingType::Props)],
        );
        // Props in inline mode: __props. prefix
        assert!(
            result.contains("__props.title"),
            "Props should get __props. prefix, got: {}",
            result
        );
    }

    // ── Structural directive removal (v-if, v-for, v-slot) ───

    /// @ai-generated — v-if attribute must be removed from JSX output
    #[test]
    fn v_if_attribute_removed_from_output() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="show">hello</div></template>"#,
            &[("show", BindingType::SetupRef)],
        );
        // Positive: IIFE if-block should be present
        assert!(
            result.contains("if(show)"),
            "v-if condition should produce IIFE if-block, got: {}",
            result
        );
        // Negative: v-if attribute must NOT appear in output
        assert!(
            !result.contains("v-if"),
            "v-if attribute must be removed from JSX output, got: {}",
            result
        );
    }

    /// @ai-generated — v-if with compound expression: attribute removed, ternary present
    #[test]
    fn v_if_compound_expr_attribute_removed() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="a || b" class="foo">hello</div></template>"#,
            &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
        );
        assert!(
            !result.contains("v-if"),
            "v-if attribute must not appear in output, got: {}",
            result
        );
        // The condition should be in the ternary
        assert!(
            result.contains("a || b"),
            "resolved condition should be in ternary, got: {}",
            result
        );
        // The class attribute should still be present
        assert!(
            result.contains(r#"class="foo""#),
            "class attribute should be preserved, got: {}",
            result
        );
    }

    /// @ai-generated — v-if with binding prefix: __props.show in IIFE, no v-if attr
    #[test]
    fn v_if_with_props_binding_attribute_removed() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="show" class="active">content</div></template>"#,
            &[("show", BindingType::Props)],
        );
        assert!(
            !result.contains("v-if"),
            "v-if must be removed from output, got: {}",
            result
        );
        assert!(
            result.contains("if(__props.show)"),
            "should have __props.show in if-condition, got: {}",
            result
        );
        // v-if value should NOT appear as string attribute value
        assert!(
            !result.contains(r#"="show""#) && !result.contains(r#"="__props.show""#),
            "v-if value should not be in attribute quotes, got: {}",
            result
        );
    }

    /// @ai-generated — v-else-if attribute must be removed
    #[test]
    fn v_else_if_attribute_removed() {
        let result = gen_tsx_template(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        );
        assert!(
            !result.contains("v-if"),
            "v-if must not appear in output, got: {}",
            result
        );
        assert!(
            !result.contains("v-else-if"),
            "v-else-if must not appear in output, got: {}",
            result
        );
        assert!(
            !result.contains("v-else"),
            "v-else must not appear in output, got: {}",
            result
        );
    }

    /// @ai-generated — v-for attribute must be removed, .map() wrapper present
    #[test]
    fn v_for_attribute_removed_from_output() {
        let result = gen_tsx_template(
            r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#,
        );
        assert!(
            !result.contains("v-for"),
            "v-for attribute must be removed from JSX output, got: {}",
            result
        );
        // Positive: .map() wrapper should be present
        assert!(
            result.contains(".map("),
            "v-for should produce .map() wrapper, got: {}",
            result
        );
        // The " in " separator should not appear as raw text
        assert!(
            !result.contains(r#""item in items""#),
            "v-for expression should not appear as attribute value string, got: {}",
            result
        );
    }

    /// @ai-generated — v-for with props binding: iterable gets prefix
    #[test]
    fn v_for_with_props_binding_attribute_removed() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><li v-for="item in list">{{ item }}</li></template>"#,
            &[("list", BindingType::Props)],
        );
        assert!(
            !result.contains("v-for"),
            "v-for must be removed from output, got: {}",
            result
        );
        assert!(
            result.contains("__props.list.map("),
            "iterable should get __props. prefix, got: {}",
            result
        );
    }

    /// @ai-generated — v-slot attribute must be removed
    #[test]
    fn v_slot_attribute_removed_from_output() {
        let result = gen_tsx_template(
            r#"<template><MyComp><template #default="{ item }"><span>{{ item }}</span></template></MyComp></template>"#,
        );
        assert!(
            !result.contains("v-slot") && !result.contains("#default"),
            "v-slot/#default must be removed from output, got: {}",
            result
        );
    }

    /// @ai-generated — v-once attribute must be removed
    #[test]
    fn v_once_attribute_removed_from_output() {
        let result = gen_tsx_template(r#"<template><div v-once>static content</div></template>"#);
        assert!(
            !result.contains("v-once"),
            "v-once must be removed from JSX output, got: {}",
            result
        );
        assert!(
            result.contains("<div>"),
            "element should still be present, got: {}",
            result
        );
    }

    /// @ai-generated — multiple directives on same element: all removed
    #[test]
    fn multiple_directives_all_removed() {
        let result = gen_tsx_template(
            r#"<template><div v-if="show" v-once class="box">hello</div></template>"#,
        );
        assert!(
            !result.contains("v-if"),
            "v-if must be removed, got: {}",
            result
        );
        assert!(
            !result.contains("v-once"),
            "v-once must be removed, got: {}",
            result
        );
        assert!(
            result.contains(r#"class="box""#),
            "regular attributes should be preserved, got: {}",
            result
        );
    }

    /// @ai-generated — v-if and v-for on same element: both removed
    #[test]
    fn v_if_and_v_for_on_same_element_both_removed() {
        let result = gen_tsx_template(
            r#"<template><div v-for="item in items" v-if="item.active">{{ item.name }}</div></template>"#,
        );
        assert!(
            !result.contains("v-for"),
            "v-for must be removed, got: {}",
            result
        );
        assert!(
            !result.contains("v-if"),
            "v-if must be removed, got: {}",
            result
        );
        assert!(
            result.contains(".map("),
            "should have .map() wrapper, got: {}",
            result
        );
        assert!(
            result.contains("?"),
            "should have ternary from v-if (not IIFE), got: {}",
            result
        );
        assert!(
            result.contains(": null"),
            "should have ternary null branch, got: {}",
            result
        );
    }

    // ── v-for comprehensive tests ────────────────────────────────

    /// @ai-generated — v-for with destructured params and index
    #[test]
    fn v_for_destructured_params() {
        let result = gen_tsx_template(
            r#"<template><li v-for="(item, index) in items" :key="index">{{ item }}</li></template>"#,
        );
        assert!(
            !result.contains("v-for"),
            "v-for attribute must be removed, got: {}",
            result
        );
        assert!(
            result.contains(".map((item, index)"),
            "destructured params should be in .map() callback, got: {}",
            result
        );
        // " in " separator must not appear as raw text
        assert!(
            !result.contains("\" in \"") && !result.contains(" in items"),
            "v-for separator must not appear in output, got: {}",
            result
        );
    }

    /// @ai-generated — v-for with object triple destructure
    #[test]
    fn v_for_object_destructure() {
        let result = gen_tsx_template(
            r#"<template><div v-for="(value, key, index) in obj">{{ key }}: {{ value }}</div></template>"#,
        );
        assert!(
            !result.contains("v-for"),
            "v-for must be removed, got: {}",
            result
        );
        assert!(
            result.contains(".map((value, key, index)"),
            "triple destructure should be in .map(), got: {}",
            result
        );
    }

    /// @ai-generated — v-for "of" variant
    #[test]
    fn v_for_of_variant() {
        let result = gen_tsx_template(
            r#"<template><span v-for="item of items">{{ item }}</span></template>"#,
        );
        assert!(
            !result.contains("v-for"),
            "v-for must be removed, got: {}",
            result
        );
        assert!(
            result.contains(".map("),
            "should produce .map() wrapper, got: {}",
            result
        );
        // "of" separator must not leak
        assert!(
            !result.contains(" of items"),
            "v-for 'of' separator must not appear in output, got: {}",
            result
        );
    }

    /// @ai-generated — v-for with numeric range
    #[test]
    fn v_for_numeric_range() {
        let result =
            gen_tsx_template(r#"<template><span v-for="n in 10">{{ n }}</span></template>"#);
        assert!(
            !result.contains("v-for"),
            "v-for must be removed, got: {}",
            result
        );
        assert!(
            result.contains("10.map("),
            "numeric range should be iterable in .map(), got: {}",
            result
        );
    }

    /// @ai-generated — v-for with complex iterable expression
    #[test]
    fn v_for_complex_iterable_expression() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-for="item in items.filter(x => x.active)" :key="item.id">{{ item.name }}</div></template>"#,
            &[("items", BindingType::SetupConst)],
        );
        assert!(
            !result.contains("v-for"),
            "v-for must be removed, got: {}",
            result
        );
        assert!(
            result.contains(".filter("),
            "complex iterable expression should be preserved, got: {}",
            result
        );
        assert!(
            result.contains(".map("),
            "should have .map() wrapper, got: {}",
            result
        );
    }

    /// @ai-generated — v-for with setup ref iterable gets binding prefix
    #[test]
    fn v_for_setup_ref_iterable_binding() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><li v-for="item in todos">{{ item.text }}</li></template>"#,
            &[("todos", BindingType::SetupRef)],
        );
        assert!(
            !result.contains("v-for"),
            "v-for must be removed, got: {}",
            result
        );
        assert!(
            result.contains("todos.map(") && !result.contains("todos.value"),
            "SetupRef iterable should be bare identifier in TSX mode (no .value), got: {}",
            result
        );
    }

    /// @ai-generated — v-for closing produces ))}, not raw text
    #[test]
    fn v_for_closing_structure() {
        let result = gen_tsx_template(
            r#"<template><div v-for="item in items" :key="item.id">text</div></template>"#,
        );
        assert!(
            result.contains("))}"),
            "v-for closing should produce CloseParen+CloseParen+CloseBrace for .map() closure, got: {}",
            result
        );
    }

    // ── ref attribute tests ──────────────────────────────────────

    /// @ai-generated — static ref attribute converts to JSX expression
    #[test]
    fn ref_static_converts_to_jsx_expression() {
        let result = gen_tsx_template(r#"<template><div ref="myRef">content</div></template>"#);
        // Should convert to ref={"myRef"} (JSX expression with string literal)
        assert!(
            result.contains(r#"ref={"myRef"}"#),
            "static ref should become ref={{\"myRef\"}}, got: {}",
            result
        );
        // Must NOT have bare ref="myRef" (Vue syntax, not valid JSX expression)
        assert!(
            !result.contains(r#"ref="myRef""#),
            "bare ref=\"myRef\" must not appear in JSX output, got: {}",
            result
        );
    }

    /// @ai-generated — dynamic :ref binding converts to JSX expression
    #[test]
    fn ref_dynamic_binding_converts_to_jsx_expression() {
        let result = gen_tsx_template(
            r#"<template><div :ref="el => (myRef = el)">content</div></template>"#,
        );
        assert!(
            result.contains("ref={"),
            "dynamic :ref should become ref={{expr}}, got: {}",
            result
        );
        // The :ref prefix must be removed
        assert!(
            !result.contains(":ref"),
            ":ref prefix must not appear in output, got: {}",
            result
        );
    }

    /// @ai-generated — ref with other attributes preserved
    #[test]
    fn ref_with_other_attrs_preserved() {
        let result = gen_tsx_template(
            r#"<template><input ref="inputRef" type="text" class="field" /></template>"#,
        );
        assert!(
            result.contains(r#"ref={"inputRef"}"#),
            "ref should be converted, got: {}",
            result
        );
        assert!(
            result.contains(r#"type="text""#),
            "type attribute should be preserved, got: {}",
            result
        );
        assert!(
            result.contains(r#"class="field""#),
            "class attribute should be preserved, got: {}",
            result
        );
    }

    // ── v-if IIFE structure tests ─────────────────────────────

    /// @ai-generated — v-if produces IIFE with if-block
    #[test]
    fn v_if_iife_structure() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="visible">hello</div></template>"#,
            &[("visible", BindingType::SetupRef)],
        );
        // Must have IIFE pattern: {()=>{if(cond){...}}}
        assert!(
            result.contains("{()=>{if(visible){"),
            "v-if should open with IIFE if-block, got: {}",
            result
        );
        // Must close with }}} (block close + arrow body close + JSX expression close)
        assert!(
            result.contains("}}}"),
            "v-if standalone should close with }}}}, got: {}",
            result
        );
        // Must NOT have ternary pattern
        assert!(
            !result.contains("? ("),
            "should not use ternary pattern, got: {}",
            result
        );
        assert!(
            !result.contains(": null}"),
            "should not have null fallback, got: {}",
            result
        );
    }

    /// @ai-generated — v-if/v-else-if/v-else produces IIFE with if/else-if/else chain
    #[test]
    fn v_if_else_chain_iife_structure() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
            &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
        );
        // Should have IIFE if/else-if/else chain
        assert!(
            result.contains("{()=>{if(a){"),
            "should have IIFE if-block, got: {}",
            result
        );
        assert!(
            result.contains("else if(b){"),
            "should have else-if block, got: {}",
            result
        );
        assert!(
            result.contains("else{"),
            "should have else block, got: {}",
            result
        );
        // Should close with }}} at the end (else block close + arrow body + JSX)
        assert!(
            result.contains("}}}"),
            "chain should close properly, got: {}",
            result
        );
        // Should NOT have standalone "v-else" text
        assert!(
            !result.contains("v-else"),
            "v-else must not appear as attribute, got: {}",
            result
        );
    }

    /// @ai-generated — v-if/v-else-if without v-else closes properly
    #[test]
    fn v_if_else_if_without_else_closes() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div></template>"#,
            &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
        );
        assert!(
            result.contains("{()=>{if(a){"),
            "should have IIFE if-block, got: {}",
            result
        );
        assert!(
            result.contains("else if(b){"),
            "should have else-if block, got: {}",
            result
        );
        // Without v-else, parent loop adds }}
        assert!(
            result.contains("}}}"),
            "chain without else should close with }}}}, got: {}",
            result
        );
    }

    /// @ai-generated — v-if with binding prefix in IIFE
    #[test]
    fn v_if_with_binding_prefix_iife() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="show">content</div></template>"#,
            &[("show", BindingType::Props)],
        );
        assert!(
            result.contains("{()=>{if(__props.show){"),
            "should use __props.show in if-condition, got: {}",
            result
        );
    }

    // ── v-if prop narrowing guard tests ──────────────────────────

    /// @ai-generated — v-if with event handler ($event) gets guard in callback
    #[test]
    fn v_if_event_handler_gets_guard() {
        let result = gen_tsx_template(
            r#"<template><div v-if="show" @click="handler($event)">click</div></template>"#,
        );
        // Event handler with $event should have guard: if (!(...)) { return undefined; }
        assert!(
            result.contains("return undefined"),
            "event handler in v-if should have narrowing guard, got: {}",
            result
        );
        assert!(
            result.contains("show"),
            "guard should reference the condition, got: {}",
            result
        );
        // Positive: still has the event handler
        assert!(
            result.contains("onClick={"),
            "should have onClick handler, got: {}",
            result
        );
        // Negative: v-if should not appear
        assert!(
            !result.contains("v-if"),
            "v-if must be removed, got: {}",
            result
        );
    }

    /// @ai-generated — v-else-if event handler gets combined guard (negation + own)
    #[test]
    fn v_else_if_event_handler_gets_combined_guard() {
        let result = gen_tsx_template(
            r#"<template><div v-if="a">A</div><div v-else-if="b" @click="handler($event)">B</div></template>"#,
        );
        // Guard should negate prior siblings: !((a)) and include own condition (b)
        assert!(
            result.contains("!(("),
            "guard should have negation of prior condition, got: {}",
            result
        );
    }

    /// @ai-generated — v-if non-function prop gets no guard
    #[test]
    fn v_if_non_function_prop_no_guard() {
        let result = gen_tsx_template(
            r#"<template><div v-if="show" :class="myClass">content</div></template>"#,
        );
        // Non-function bindings should NOT have guards
        assert!(
            !result.contains("?undefined:"),
            "non-function prop should not have ternary guard, got: {}",
            result
        );
    }

    // ── v-if nested IIFE tests ──────────────────────────────────

    /// @ai-generated — nested v-if gets block guard from parent scope
    #[test]
    fn v_if_nested_gets_block_guard() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="parent"><span v-if="child">nested</span></div></template>"#,
            &[
                ("parent", BindingType::SetupRef),
                ("child", BindingType::SetupRef),
            ],
        );
        // Nested v-if should have block guard: if(!(condText)) return;
        let has_guard = result.contains("return;") && result.contains("if(!(");
        assert!(
            has_guard,
            "nested v-if should have block guard from parent, got: {}",
            result
        );
        // Should still have the nested if-condition
        assert!(
            result.contains("if(child)"),
            "nested v-if should have its own if-condition, got: {}",
            result
        );
    }

    // ── Part F: Comment repositioning ────────────────────────────────

    #[test]
    fn v_if_comment_before_repositioned_inside_iife() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><!-- @ts-expect-error --><div v-if="show">hello</div></template>"#,
            &[("show", BindingType::SetupRef)],
        );
        // Comment should appear INSIDE the IIFE, after the if(cond){ line
        // Pattern: {()=>{if(cond){ {/* @ts-expect-error */} <div>...
        assert!(
            result.contains("if(show)"),
            "should have IIFE condition, got:\n{}",
            result
        );
        // Comment must be AFTER the IIFE open, not before it
        let iife_pos = result.find("{()=>{").expect("should have IIFE open");
        let comment_pos = result
            .find("{/* @ts-expect-error */}")
            .expect("comment should be preserved");
        assert!(
            comment_pos > iife_pos,
            "comment should appear AFTER IIFE open, got:\n{}",
            result
        );
        // Negative: comment should NOT appear before the IIFE
        let before_iife = &result[..iife_pos];
        assert!(
            !before_iife.contains("@ts-expect-error"),
            "comment must not appear before IIFE, got:\n{}",
            result
        );
    }

    #[test]
    fn v_if_without_preceding_comment_no_change() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-if="show">hello</div></template>"#,
            &[("show", BindingType::SetupRef)],
        );
        // No comment to reposition — should work normally
        assert!(
            result.contains("{()=>{if(show){"),
            "should have IIFE pattern, got:\n{}",
            result
        );
        assert!(
            !result.contains("{/*"),
            "should not have any comments, got:\n{}",
            result
        );
    }

    // ── Part F2: v-if/v-else with whitespace between elements ────────

    /// @ai-generated — v-if + whitespace text + v-else must stay in single IIFE chain
    #[test]
    fn v_if_else_with_whitespace_between_elements() {
        // Simulates formatted template: <img v-if="cond" />\n  <span v-else>fallback</span>
        let result = gen_tsx_template_with_bindings(
            "<template>\n  <img v-if=\"show\" />\n  <span v-else>fallback</span>\n</template>",
            &[("show", BindingType::SetupRef)],
        );

        // Positive: must have complete IIFE chain with if/else
        assert!(
            result.contains("{()=>{if(show){"),
            "should have IIFE if-block, got:\n{}",
            result
        );
        assert!(
            result.contains("else{"),
            "should have else block in same IIFE, got:\n{}",
            result
        );

        // Structural: IIFE must NOT close before else — no }}} between IIFE start and else
        let iife_start = result.find("{()=>{if(").unwrap();
        let else_pos = result.find("else{").unwrap();
        let between = &result[iife_start..else_pos];
        assert!(
            !between.contains("}}}"),
            "IIFE must not close before else: premature close found between IIFE start and else, got:\n{}",
            result
        );

        // Negative: v-if/v-else attributes must not appear in output
        assert!(
            !result.contains("v-if"),
            "v-if attribute must be removed from JSX, got:\n{}",
            result
        );
        assert!(
            !result.contains("v-else"),
            "v-else attribute must be removed from JSX, got:\n{}",
            result
        );

        // Validate JSX syntax: the full template output must parse
        let wrapper = format!("const x = {}", result);
        let val_alloc = oxc_allocator::Allocator::new();
        let parsed =
            oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
        assert!(
            parsed.errors.is_empty(),
            "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
            parsed
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>(),
            result
        );
    }

    /// @ai-generated — v-if/v-else-if/v-else with formatted whitespace stays in single IIFE
    #[test]
    fn v_if_else_if_else_with_whitespace() {
        let result = gen_tsx_template_with_bindings(
            "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else-if=\"b\">B</div>\n  <div v-else>C</div>\n</template>",
            &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
        );

        // Positive: complete IIFE chain
        assert!(
            result.contains("{()=>{if(a){"),
            "should have IIFE if-block, got:\n{}",
            result
        );
        assert!(
            result.contains("else if(b){"),
            "should have else-if block, got:\n{}",
            result
        );
        assert!(
            result.contains("else{"),
            "should have else block, got:\n{}",
            result
        );

        // Structural: IIFE must NOT close before else-if or else
        let iife_start = result.find("{()=>{if(").unwrap();
        let else_if_pos = result.find("else if(").unwrap();
        let else_pos = result.find("else{").unwrap();
        let between_if_and_else_if = &result[iife_start..else_if_pos];
        assert!(
            !between_if_and_else_if.contains("}}}"),
            "IIFE must not close before else-if, got:\n{}",
            result
        );
        let between_else_if_and_else = &result[else_if_pos..else_pos];
        assert!(
            !between_else_if_and_else.contains("}}}"),
            "IIFE must not close before else, got:\n{}",
            result
        );

        // Negative: directive attributes must not appear
        assert!(
            !result.contains("v-if"),
            "v-if must be removed, got:\n{}",
            result
        );
        assert!(
            !result.contains("v-else"),
            "v-else must be removed, got:\n{}",
            result
        );

        // Validate JSX syntax: the full template output must parse
        let wrapper = format!("const x = {}", result);
        let val_alloc = oxc_allocator::Allocator::new();
        let parsed =
            oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
        assert!(
            parsed.errors.is_empty(),
            "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
            parsed
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>(),
            result
        );
    }

    // ── Part G: <template v-slot> with v-if ──────────────────────────

    #[test]
    fn template_v_if_v_slot_skips_iife() {
        // <template v-if v-slot> should NOT get IIFE wrapping (slot handles conditions)
        let result = gen_tsx_template(
            r#"<template><MyComp><template v-if="show" #default>content</template></MyComp></template>"#,
        );
        // The IIFE pattern should NOT wrap the slot template
        assert!(
            !result.contains("{()=>{if(show){"),
            "template with v-if + v-slot should not get IIFE wrapping, got:\n{}",
            result
        );
    }

    // ── Part C Step 8: v-bind function prop guards ──────────────────

    #[test]
    fn v_bind_arrow_expr_gets_ternary_guard() {
        // Arrow expression body: `:handler="() => msg.trim()"` inside v-if
        // → handler={() => !(guard)?undefined:msg.trim()}
        let result = gen_tsx_template(
            r#"<template><div v-if="typeof msg === 'string'" :handler="() => msg.trim()">hi</div></template>"#,
        );
        let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            norm.contains("?undefined:"),
            "arrow expression prop should get ternary guard, got:\n{}",
            result
        );
        assert!(
            !norm.contains("if(!(") || norm.contains("{()=>{if("),
            "arrow expression should use ternary guard, not block guard in handler, got:\n{}",
            result
        );
    }

    #[test]
    fn v_bind_arrow_block_gets_block_guard() {
        // Arrow block body: `:handler="() => { return msg.trim() }"` inside v-if
        // → handler={() => {if(!(guard))return; return msg.trim() }}
        let result = gen_tsx_template(
            r#"<template><div v-if="typeof msg === 'string'" :handler="() => { return msg.trim() }">hi</div></template>"#,
        );
        let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
        // The handler value should contain a block guard
        // Find the handler= part and check for block guard inside it
        let handler_pos = norm.find("handler={").expect("should have handler prop");
        let after_handler = &norm[handler_pos..];
        assert!(
            after_handler.contains("if(!(") && after_handler.contains(")return;"),
            "arrow block prop should get block guard inside handler, got:\n{}",
            result
        );
    }

    #[test]
    fn v_bind_function_expr_gets_block_guard() {
        // Function expression: `:handler="function() { return msg.trim() }"` inside v-if
        // → handler={function() {if(!(guard))return; return msg.trim() }}
        let result = gen_tsx_template(
            r#"<template><div v-if="typeof msg === 'string'" :handler="function() { return msg.trim() }">hi</div></template>"#,
        );
        let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
        let handler_pos = norm.find("handler={").expect("should have handler prop");
        let after_handler = &norm[handler_pos..];
        assert!(
            after_handler.contains("if(!(") && after_handler.contains(")return;"),
            "function expression prop should get block guard, got:\n{}",
            result
        );
    }

    #[test]
    fn v_bind_non_function_no_guard() {
        // Non-function props should NOT get any guard
        let result =
            gen_tsx_template(r#"<template><div v-if="show" :class="msg">hi</div></template>"#);
        let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
        // Find the class prop
        let class_pos = norm.find("class={").expect("should have class prop");
        let after_class = &norm[class_pos..];
        // Should NOT have any guard
        assert!(
            !after_class.starts_with("class={()=>") && !after_class.contains("?undefined:"),
            "non-function prop should not get guard, got:\n{}",
            result
        );
    }

    // ── Part H: JSX syntax validation for directive combinations ─────

    /// Validate that the generated TSX template is parseable JSX/TSX.
    /// Wraps the template output in a JSX fragment so IIFE expressions parse correctly.
    fn assert_valid_jsx(source: &str, label: &str) {
        let result = gen_tsx_template(source);
        let wrapper = format!("const x = <>{}</>", result);
        let val_alloc = oxc_allocator::Allocator::new();
        let parsed =
            oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
        assert!(
            parsed.errors.is_empty(),
            "[{}] TSX syntax errors: {:?}\n--- source ---\n{}\n--- output ---\n{}",
            label,
            parsed
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>(),
            source,
            result
        );
    }

    /// @ai-generated — v-if alone produces valid JSX
    #[test]
    fn jsx_valid_v_if_alone() {
        assert_valid_jsx(
            r#"<template><div v-if="show">content</div></template>"#,
            "v-if alone",
        );
    }

    /// @ai-generated — v-if/v-else produces valid JSX
    #[test]
    fn jsx_valid_v_if_else() {
        assert_valid_jsx(
            r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#,
            "v-if/v-else inline",
        );
    }

    /// @ai-generated — v-if/v-else-if/v-else chain produces valid JSX
    #[test]
    fn jsx_valid_v_if_else_if_else() {
        assert_valid_jsx(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
            "v-if/v-else-if/v-else inline",
        );
    }

    /// @ai-generated — v-if/v-else with whitespace formatting produces valid JSX
    #[test]
    fn jsx_valid_v_if_else_whitespace() {
        assert_valid_jsx(
            "<template>\n  <div v-if=\"show\">A</div>\n  <div v-else>B</div>\n</template>",
            "v-if/v-else with whitespace",
        );
    }

    /// @ai-generated — full v-if chain with whitespace produces valid JSX
    #[test]
    fn jsx_valid_v_if_else_if_else_whitespace() {
        assert_valid_jsx(
            "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else-if=\"b\">B</div>\n  <div v-else>C</div>\n</template>",
            "v-if/v-else-if/v-else with whitespace",
        );
    }

    /// @ai-generated — v-for alone produces valid JSX
    #[test]
    fn jsx_valid_v_for_alone() {
        assert_valid_jsx(
            r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#,
            "v-for alone",
        );
    }

    /// @ai-generated — v-for with index produces valid JSX
    #[test]
    fn jsx_valid_v_for_with_index() {
        assert_valid_jsx(
            r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
            "v-for with index",
        );
    }

    /// @ai-generated — v-slot on component produces valid JSX
    #[test]
    fn jsx_valid_v_slot_component() {
        assert_valid_jsx(
            r#"<template><MyComp v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
            "v-slot on component",
        );
    }

    /// @ai-generated — named slots with template produces valid JSX
    #[test]
    fn jsx_valid_named_slot() {
        assert_valid_jsx(
            r#"<template><MyComp><template #header>Header</template><template #default>Body</template></MyComp></template>"#,
            "named slots with template",
        );
    }

    /// @ai-generated — v-if + v-for on same element produces valid JSX
    #[test]
    fn jsx_valid_v_if_v_for_same_element() {
        assert_valid_jsx(
            r#"<template><div v-if="show" v-for="item in items" :key="item">{{ item }}</div></template>"#,
            "v-if + v-for same element",
        );
    }

    /// @ai-generated — v-for containing v-if/v-else children produces valid JSX
    #[test]
    fn jsx_valid_v_for_with_v_if_children() {
        assert_valid_jsx(
            r#"<template><ul><li v-for="item in items" :key="item.id"><span v-if="item.active">active</span><span v-else>inactive</span></li></ul></template>"#,
            "v-for with v-if/v-else children",
        );
    }

    /// @ai-generated — v-for with v-if/v-else children and whitespace produces valid JSX
    #[test]
    fn jsx_valid_v_for_with_v_if_children_whitespace() {
        assert_valid_jsx(
            "<template>\n  <ul>\n    <li v-for=\"item in items\" :key=\"item.id\">\n      <span v-if=\"item.active\">active</span>\n      <span v-else>inactive</span>\n    </li>\n  </ul>\n</template>",
            "v-for with v-if/v-else children whitespace",
        );
    }

    /// @ai-generated — component with v-if and v-slot produces valid JSX
    #[test]
    fn jsx_valid_v_if_with_v_slot() {
        assert_valid_jsx(
            r#"<template><MyComp v-if="show" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
            "v-if + v-slot on component",
        );
    }

    /// @ai-generated — v-for of components with v-slot produces valid JSX
    #[test]
    fn jsx_valid_v_for_with_v_slot() {
        assert_valid_jsx(
            r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
            "v-for + v-slot",
        );
    }

    /// @ai-generated — nested v-if chains produce valid JSX
    #[test]
    fn jsx_valid_nested_v_if() {
        assert_valid_jsx(
            r#"<template><div v-if="a"><span v-if="b">B</span><span v-else>not B</span></div></template>"#,
            "nested v-if chains",
        );
    }

    /// @ai-generated — v-if with template v-for inside produces valid JSX
    #[test]
    fn jsx_valid_v_if_with_template_v_for() {
        assert_valid_jsx(
            "<template>\n  <div v-if=\"show\">\n    <span v-for=\"item in items\" :key=\"item\">{{ item }}</span>\n  </div>\n  <div v-else>empty</div>\n</template>",
            "v-if with v-for inside + v-else",
        );
    }

    /// @ai-generated — multiple separate v-if chains produce valid JSX
    #[test]
    fn jsx_valid_multiple_v_if_chains() {
        assert_valid_jsx(
            "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else>not A</div>\n  <div v-if=\"b\">B</div>\n  <div v-else>not B</div>\n</template>",
            "multiple separate v-if chains with whitespace",
        );
    }

    /// @ai-generated — v-for + v-if + v-slot all together produces valid JSX
    #[test]
    fn jsx_valid_all_directives_combined() {
        assert_valid_jsx(
            "<template>\n  <div v-if=\"hasItems\">\n    <MyComp v-for=\"item in items\" :key=\"item.id\" v-slot=\"{ row }\">\n      <span v-if=\"row.active\">{{ row.name }}</span>\n      <span v-else>inactive</span>\n    </MyComp>\n  </div>\n  <div v-else>no items</div>\n</template>",
            "v-if + v-for + v-slot + nested v-if/v-else",
        );
    }

    // ===================================================================
    // @ai-generated — v-show binding resolution tests
    // ===================================================================

    #[test]
    fn v_show_with_ref_binding_gets_prefix() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-show="visible">hi</div></template>"#,
            &[("visible", BindingType::SetupRef)],
        );
        assert!(
            result.contains("visible") && !result.contains("visible.value"),
            "v-show ref binding should be bare identifier in TSX mode (no .value). Got: {}",
            result
        );
        assert!(
            result.contains("display:"),
            "v-show should produce style display. Got: {}",
            result
        );
        assert!(
            !result.contains("v-show"),
            "v-show attribute must be removed. Got: {}",
            result
        );
    }

    #[test]
    fn v_show_with_props_binding_gets_prefix() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-show="isVisible">hi</div></template>"#,
            &[("isVisible", BindingType::Props)],
        );
        assert!(
            result.contains("__props.isVisible"),
            "v-show props binding should have __props. prefix. Got: {}",
            result
        );
    }

    #[test]
    fn v_show_compound_expr_resolves_all_bindings() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div v-show="isAdmin && visible">hi</div></template>"#,
            &[
                ("isAdmin", BindingType::Props),
                ("visible", BindingType::SetupRef),
            ],
        );
        assert!(
            result.contains("__props.isAdmin"),
            "v-show should resolve isAdmin as props. Got: {}",
            result
        );
        assert!(
            result.contains("visible") && !result.contains("visible.value"),
            "v-show should resolve visible as bare identifier in TSX mode (no .value). Got: {}",
            result
        );
    }

    // ── v-model in TSX ────────────────────────────────────────────

    #[test]
    fn v_model_basic_component() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><Comp v-model="count" /></template>"#,
            &[("count", BindingType::SetupRef)],
        );
        assert!(
            result.contains("modelValue={count}"),
            "v-model should produce modelValue prop. Got: {}",
            result
        );
        assert!(
            result.contains("\"onUpdate:modelValue\""),
            "v-model should produce onUpdate:modelValue handler. Got: {}",
            result
        );
        // Must use spread syntax (bare quoted attribute is invalid JSX)
        assert!(
            !result.contains("\"onUpdate:modelValue\"={"),
            "onUpdate handler must NOT be a bare JSX attribute. Got: {}",
            result
        );
        assert!(
            !result.contains("v-model"),
            "v-model attribute must be removed from JSX. Got: {}",
            result
        );
    }

    #[test]
    fn v_model_named() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><Comp v-model:title="title" /></template>"#,
            &[("title", BindingType::SetupRef)],
        );
        assert!(
            result.contains("title={title}"),
            "named v-model should produce named prop. Got: {}",
            result
        );
        assert!(
            result.contains("\"onUpdate:title\""),
            "named v-model should produce onUpdate:title handler. Got: {}",
            result
        );
        // Must use spread syntax (bare quoted attribute is invalid JSX)
        assert!(
            !result.contains("\"onUpdate:title\"={"),
            "named onUpdate handler must NOT be a bare JSX attribute. Got: {}",
            result
        );
    }

    #[test]
    fn v_model_with_binding_resolution() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><Comp v-model="count" /></template>"#,
            &[("count", BindingType::SetupRef)],
        );
        assert!(
            result.contains("modelValue={count}") && !result.contains("count.value"),
            "v-model on ref should resolve to bare identifier in TSX mode (no .value). Got: {}",
            result
        );
    }

    #[test]
    fn v_model_on_native_element() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><input v-model="msg" /></template>"#,
            &[("msg", BindingType::SetupRef)],
        );
        // Native input should use `value` (not `modelValue`) and native event handler
        assert!(
            result.contains("value={msg}"),
            "v-model on native input should produce value prop. Got: {}",
            result
        );
        assert!(
            !result.contains("modelValue"),
            "v-model on native input must NOT use modelValue. Got: {}",
            result
        );
        assert!(
            result.contains("onInput={"),
            "v-model on native input should use onInput event. Got: {}",
            result
        );
        // Must not have any quoted attribute names (invalid JSX)
        assert!(
            !result.contains(r#""onUpdate:"#),
            "native input must not have quoted onUpdate attribute. Got: {}",
            result
        );
        assert!(
            !result.contains("v-model"),
            "v-model attribute must be removed. Got: {}",
            result
        );
    }

    // ── Slot outlets in TSX ────────────────────────────────────────

    #[test]
    fn slot_outlet_default() {
        let result = gen_tsx_template(r#"<template><slot /></template>"#);
        assert!(
            result.contains("___VERTER___instance.$slots.default?.()"),
            "Default slot outlet should produce ___VERTER___instance.$slots.default?.(). Got: {}",
            result
        );
        assert!(
            !result.contains("<slot"),
            "<slot> tag must be replaced. Got: {}",
            result
        );
        assert!(
            !result.contains("{ $slots.default"),
            "Bare $slots without instance prefix must not appear. Got: {}",
            result
        );
    }

    #[test]
    fn slot_outlet_named() {
        let result = gen_tsx_template(r#"<template><slot name="header" /></template>"#);
        assert!(
            result.contains("___VERTER___instance.$slots.header?.()"),
            "Named slot outlet should produce ___VERTER___instance.$slots.header?.(). Got: {}",
            result
        );
        assert!(
            !result.contains("{ $slots.header"),
            "Bare $slots without instance prefix must not appear. Got: {}",
            result
        );
    }

    #[test]
    fn slot_outlet_with_props() {
        let result =
            gen_tsx_template(r#"<template><slot name="item" :data="itemData" /></template>"#);
        assert!(
            result.contains("___VERTER___instance.$slots.item"),
            "Slot call should reference ___VERTER___instance.$slots.item. Got: {}",
            result
        );
        assert!(
            result.contains("data: ___VERTER___instance.itemData")
                || result.contains("data:___VERTER___instance.itemData"),
            "Slot props should include data binding with instance prefix (unresolved). Got: {}",
            result
        );
    }

    #[test]
    fn slot_outlet_with_fallback() {
        let result = gen_tsx_template(r#"<template><slot>fallback</slot></template>"#);
        assert!(
            result.contains("___VERTER___instance.$slots.default?.()"),
            "Slot with fallback should have ___VERTER___instance.$slots call. Got: {}",
            result
        );
        assert!(
            result.contains("??"),
            "Slot with fallback should use ?? operator. Got: {}",
            result
        );
    }

    // ── Instance property resolution in TSX ─────────────────────────

    #[test]
    fn tsx_unresolved_dollar_emit_gets_instance_prefix() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div>{{ $emit('click') }}</div></template>"#,
            &[],
        );
        assert!(
            result.contains("___VERTER___instance.$emit"),
            "Unresolved $emit should get instance prefix. Got: {}",
            result
        );
        assert!(
            !result.contains("{ $emit(") && !result.contains("{$emit("),
            "Bare $emit without prefix must not appear. Got: {}",
            result
        );
    }

    #[test]
    fn tsx_unresolved_dollar_attrs_gets_instance_prefix() {
        let result =
            gen_tsx_template_with_bindings(r#"<template><div>{{ $attrs }}</div></template>"#, &[]);
        assert!(
            result.contains("___VERTER___instance.$attrs"),
            "Unresolved $attrs should get instance prefix. Got: {}",
            result
        );
    }

    #[test]
    fn tsx_known_setup_binding_stays_bare() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div>{{ count }}</div></template>"#,
            &[("count", BindingType::SetupRef)],
        );
        assert!(
            !result.contains("___VERTER___instance.count"),
            "Known binding should NOT get instance prefix. Got: {}",
            result
        );
    }

    #[test]
    fn tsx_props_binding_stays_dunder_props() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div>{{ msg }}</div></template>"#,
            &[("msg", BindingType::Props)],
        );
        assert!(
            result.contains("__props.msg"),
            "Props binding should use __props. Got: {}",
            result
        );
        assert!(
            !result.contains("___VERTER___instance.msg"),
            "Props binding should NOT get instance prefix. Got: {}",
            result
        );
    }

    // ── Dynamic event names in TSX ────────────────────────────────

    #[test]
    fn dynamic_event_name() {
        let result = gen_tsx_template(r#"<template><div @[eventName]="handler" /></template>"#);
        assert!(
            result.contains("eventName") || result.contains("_ctx.eventName"),
            "Dynamic event should reference eventName. Got: {}",
            result
        );
        assert!(
            !result.contains("@["),
            "Dynamic event syntax must be removed. Got: {}",
            result
        );
    }

    #[test]
    fn dynamic_event_name_with_binding() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div @[eventName]="handler" /></template>"#,
            &[("eventName", BindingType::SetupRef)],
        );
        assert!(
            result.contains("eventName") && !result.contains("eventName.value"),
            "Dynamic event name on ref should be bare identifier in TSX mode (no .value). Got: {}",
            result
        );
    }

    // ── v-for source mapping (#19) ──────────────────────────────────

    /// Helper: generate TSX template with bindings AND return source map tokens.
    /// Returns (output_string, Vec<(dst_line, dst_col, src_col)>).
    fn gen_tsx_template_with_map(
        source: &str,
        bindings: &[(&str, BindingType)],
    ) -> (String, Vec<(u32, u32, u32)>) {
        let alloc = Allocator::new();
        let bytes = source.as_bytes();

        let mut syntax = crate::parser::Syntax::new(false);
        crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
            syntax.handle(
                &e,
                &crate::diagnostics::SyntaxPluginContext {
                    input: source,
                    bytes,
                    options: &crate::diagnostics::SyntaxPluginOptions::default(),
                    diagnostics: Vec::new(),
                },
            )
        });

        let template_ast = match syntax.take_template_ast() {
            Some(ast) => ast,
            None => return (String::new(), Vec::new()),
        };

        let source_type = oxc_span::SourceType::tsx();
        let oxc_ast = crate::template::oxc::parse_template_expressions(
            &template_ast,
            source,
            &alloc,
            source_type,
        );

        let tpl_alloc = Allocator::new();
        let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
        let mut out = CodeGenOutput::new(&tpl_alloc);
        let binding_map: FxHashMap<&str, BindingType> = bindings.iter().copied().collect();
        let options = IdeTemplateOptions {
            self_name: "App",
            comments: true,
            is_jsx: false,
        };

        generate_ide_template(
            &template_ast,
            &oxc_ast,
            source,
            &mut out,
            &tpl_alloc,
            &binding_map,
            &options,
        );
        out.apply_to(&mut tpl_ct);

        let full = tpl_ct.build_string();
        let map = tpl_ct
            .generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<(u32, u32, u32)> = map
            .get_tokens()
            .filter(|t| t.get_source_id().is_some())
            .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
            .collect();

        (full, tokens)
    }

    #[test]
    fn v_for_iterable_is_source_mapped() {
        // v-for="item in items" — the iterable `items` in the .map() wrapper
        // should have a source map token pointing back to the original `items` position.
        let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
        let (output, tokens) = gen_tsx_template_with_map(source, &[]);

        // Verify output shape
        assert!(
            output.contains(".map("),
            "v-for should produce .map() wrapper: {output}"
        );

        // Find the byte offset of "items" in the v-for attribute value
        let items_src_offset = source.find("item in items").unwrap() + "item in ".len();

        // There should be a source map token pointing to the iterable position
        let has_iterable_token = tokens
            .iter()
            .any(|&(_, _, src_col)| src_col == items_src_offset as u32);
        assert!(
            has_iterable_token,
            "v-for iterable should have source map token at src col {}. Tokens: {:?}",
            items_src_offset, tokens
        );
    }

    #[test]
    fn v_for_param_is_source_mapped() {
        // The iteration parameter `item` in .map((item) => ...) should map back
        // to the parameter position in the v-for attribute value.
        let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
        let (output, tokens) = gen_tsx_template_with_map(source, &[]);

        assert!(
            output.contains(".map((item)"),
            "v-for should produce .map((item) => ...): {output}"
        );

        // "item" starts right after the opening quote of v-for="
        let param_src_offset = source.find("item in items").unwrap();

        let has_param_token = tokens
            .iter()
            .any(|&(_, _, src_col)| src_col == param_src_offset as u32);
        assert!(
            has_param_token,
            "v-for parameter should have source map token at src col {}. Tokens: {:?}",
            param_src_offset, tokens
        );
    }

    #[test]
    fn component_is_dynamic_expr_is_source_mapped() {
        // <component :is="currentView"> should emit a source-mapped temp variable
        // so TSGO can provide hover info on `currentView`.
        let source = r#"<template><component :is="currentView">hello</component></template>"#;
        let (output, tokens) =
            gen_tsx_template_with_map(source, &[("currentView", BindingType::SetupRef)]);

        // The output should contain the temp variable with the expression
        assert!(
            output.contains("currentView"),
            "output should contain `currentView`: {output}"
        );

        // Find the byte offset of "currentView" in the :is attribute value
        let expr_src_offset = source.find("currentView").unwrap();

        // There should be a source map token pointing back to the expression
        let has_expr_token = tokens
            .iter()
            .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
        assert!(
            has_expr_token,
            "component :is expression should have source map token at src col {}. Tokens: {:?}",
            expr_src_offset, tokens
        );
    }

    #[test]
    fn component_is_dynamic_resolves_bindings() {
        // <component :is="currentView"> with SetupRef binding should resolve
        // the expression through the BindingResolver (e.g., `currentView.value`
        // for refs in non-inline mode, or just `currentView` for inline).
        let source = r#"<template><component :is="currentView">hello</component></template>"#;
        let output =
            gen_tsx_template_with_bindings(source, &[("currentView", BindingType::SetupRef)]);

        // With inline mode (default for TSX), SetupRef bindings are used directly.
        // The expression should be present in the output (not _ctx. prefixed since inline).
        assert!(
            output.contains("currentView"),
            "output should contain resolved `currentView`: {output}"
        );
        // The `:is` attribute itself should be removed
        assert!(
            !output.contains(":is="),
            "`:is` attribute should be removed from output: {output}"
        );
        // The `component` tag should be rewritten
        assert!(
            !output.contains("<component"),
            "`<component` tag should be rewritten: {output}"
        );
    }

    #[test]
    fn component_is_dynamic_resolves_data_binding() {
        // In TSX mode, Data bindings are bare identifiers (no _ctx. prefix).
        let source = r#"<template><component :is="currentView">hello</component></template>"#;
        let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::Data)]);

        assert!(
            output.contains("currentView") && !output.contains("_ctx.currentView"),
            "Data binding should be bare identifier in TSX mode: {output}"
        );
        assert!(
            !output.contains(":is="),
            "`:is` attribute should be removed from output: {output}"
        );
    }

    #[test]
    fn event_handler_simple_ident_is_source_mapped() {
        // @click="handler" — the handler identifier should have a source map token.
        let source = r#"<template><button @click="handler">click</button></template>"#;
        let (output, tokens) =
            gen_tsx_template_with_map(source, &[("handler", BindingType::SetupConst)]);

        assert!(
            output.contains("onClick={handler}"),
            "should emit onClick={{handler}}: {output}"
        );

        // Find the byte offset of "handler" in the @click value
        let handler_src_offset = source.find("handler").unwrap();

        let has_handler_token = tokens
            .iter()
            .any(|&(_, _, src_col)| src_col == handler_src_offset as u32);
        assert!(
            has_handler_token,
            "event handler should have source map token at src col {}. Tokens: {:?}",
            handler_src_offset, tokens
        );
    }

    #[test]
    fn event_handler_fn_expr_is_source_mapped() {
        // @click="(e) => doSomething(e)" — the expression should be source-mapped.
        let source =
            r#"<template><button @click="(e) => doSomething(e)">click</button></template>"#;
        let (output, tokens) =
            gen_tsx_template_with_map(source, &[("doSomething", BindingType::SetupConst)]);

        assert!(
            output.contains("onClick={(e) => doSomething(e)}"),
            "should emit onClick with fn expr: {output}"
        );

        // Find the byte offset of the expression in the @click value
        let expr_src_offset = source.find("(e) => doSomething").unwrap();

        let has_expr_token = tokens
            .iter()
            .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
        assert!(
            has_expr_token,
            "fn expression should have source map token at src col {}. Tokens: {:?}",
            expr_src_offset, tokens
        );
    }

    #[test]
    fn event_handler_inline_expr_is_source_mapped() {
        // @click="count++" — the inline expression should be source-mapped.
        // Using SetupConst to avoid .value transformation changing the text.
        let source = r#"<template><button @click="count++">click</button></template>"#;
        let (output, tokens) =
            gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

        assert!(
            output.contains("count++"),
            "should contain the expression: {output}"
        );

        // Find byte offset of "count++" in the @click value
        let expr_src_offset = source.find("count++").unwrap();

        let has_expr_token = tokens
            .iter()
            .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
        assert!(
            has_expr_token,
            "inline expression should have source map token at src col {}. Tokens: {:?}",
            expr_src_offset, tokens
        );
    }

    // ── Bug 1: Dynamic <component :is> uses extractRenderComponent ──

    #[test]
    fn component_dynamic_is_uses_extract_render_component() {
        let source = r#"<template><component :is="'div'"></component></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("___VERTER___extractRenderComponent"),
            "should use extractRenderComponent wrapper: {output}"
        );
        assert!(
            output.contains("___VERTER___component_render"),
            "should use ___VERTER___component_render temp name: {output}"
        );
        assert!(
            output
                .contains("const ___VERTER___component_render=___VERTER___extractRenderComponent("),
            "should declare const with extractRenderComponent wrapper: {output}"
        );
        // Negative: old format should not appear
        assert!(
            !output.contains("__verter_component_render"),
            "old format __verter_component_render should not appear: {output}"
        );
        assert!(
            !output.contains("<component"),
            "<component tag should be rewritten: {output}"
        );
    }

    #[test]
    fn component_dynamic_is_expression() {
        let source = r#"<template><component :is="as || 'div'"></component></template>"#;
        let output = gen_tsx_template_with_bindings(source, &[("as", BindingType::SetupRef)]);

        assert!(
            output.contains("___VERTER___extractRenderComponent("),
            "should use extractRenderComponent: {output}"
        );
        assert!(
            output.contains("<___VERTER___component_render"),
            "should rewrite opening tag: {output}"
        );
        assert!(
            output.contains("</___VERTER___component_render>"),
            "should rewrite closing tag: {output}"
        );
    }

    #[test]
    fn component_static_is_unchanged() {
        let source = r#"<template><component is="div" tabindex="1"></component></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("<div"),
            "static is should rewrite to target tag: {output}"
        );
        assert!(
            !output.contains("extractRenderComponent"),
            "static is should not use extractRenderComponent: {output}"
        );
        assert!(
            !output.contains("<component"),
            "<component tag should be rewritten: {output}"
        );
    }

    #[test]
    fn component_dynamic_is_removes_is_directive() {
        let source = r#"<template><component :is="tag" class="foo"></component></template>"#;
        let output = gen_tsx_template_with_bindings(source, &[("tag", BindingType::SetupRef)]);

        assert!(
            output.contains("class=\"foo\""),
            "class attribute should be preserved: {output}"
        );
        assert!(
            !output.contains(":is="),
            ":is= directive should be removed: {output}"
        );
    }

    // ── Bug 2: Class/Style merge ──

    #[test]
    fn class_merge_static_and_dynamic() {
        let source = r#"<template><div class="foo" :class="{bar: true}"/></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("normalizeClass"),
            "should use normalizeClass: {output}"
        );
        assert!(
            output.contains("{bar: true}") && output.contains("\"foo\""),
            "should contain both class expressions: {output}"
        );
        // Count class= occurrences — should be exactly 1
        let class_count = output.matches("class=").count();
        assert_eq!(
            class_count, 1,
            "should have exactly 1 class= attribute, got {class_count}: {output}"
        );
    }

    #[test]
    fn class_merge_with_prop_in_between() {
        let source =
            r#"<template><div class="foo" my-random-prop="true" :class="{bar: true}"/></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("normalizeClass"),
            "should use normalizeClass: {output}"
        );
        assert!(
            output.contains("my-random-prop"),
            "should preserve other props: {output}"
        );
        let class_count = output.matches("class=").count();
        assert_eq!(
            class_count, 1,
            "should have exactly 1 class= attribute, got {class_count}: {output}"
        );
    }

    #[test]
    fn style_merge_static_and_dynamic() {
        let source = r#"<template><div style="color:red" :style="{ bg: 'blue' }"/></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("normalizeStyle"),
            "should use normalizeStyle: {output}"
        );
        let style_count = output.matches("style=").count();
        assert_eq!(
            style_count, 1,
            "should have exactly 1 style= attribute, got {style_count}: {output}"
        );
    }

    #[test]
    fn class_and_style_merge_combined() {
        let source = r#"<template><div class="a" :class="b" style="c" :style="d"/></template>"#;
        let output = gen_tsx_template_with_bindings(
            source,
            &[("b", BindingType::SetupRef), ("d", BindingType::SetupRef)],
        );

        assert!(
            output.contains("normalizeClass"),
            "should use normalizeClass: {output}"
        );
        assert!(
            output.contains("normalizeStyle"),
            "should use normalizeStyle: {output}"
        );
        let class_count = output.matches("class=").count();
        assert_eq!(
            class_count, 1,
            "should have exactly 1 class= attribute: {output}"
        );
        let style_count = output.matches("style=").count();
        assert_eq!(
            style_count, 1,
            "should have exactly 1 style= attribute: {output}"
        );
    }

    #[test]
    fn class_only_static_no_merge() {
        let source = r#"<template><div class="foo"/></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("class=\"foo\""),
            "static class should be unchanged: {output}"
        );
        assert!(
            !output.contains("normalizeClass"),
            "should not use normalizeClass for static-only: {output}"
        );
    }

    #[test]
    fn class_only_dynamic_no_merge() {
        let source = r#"<template><div :class="{bar: true}"/></template>"#;
        let output = gen_tsx_template(source);

        assert!(
            output.contains("class={{bar: true}}"),
            "dynamic-only class should be simple binding: {output}"
        );
        assert!(
            !output.contains("normalizeClass"),
            "should not use normalizeClass for dynamic-only: {output}"
        );
    }
}
