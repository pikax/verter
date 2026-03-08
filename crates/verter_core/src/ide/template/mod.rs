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
        directives::emit_v_for_open(
            el,
            oxc_el,
            ctx.source,
            ctx.out,
            ctx.alloc,
            ctx.resolver,
            ctx.options.is_jsx,
        );
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

    // Track whether dynamic <component :is> needs IIFE closing after element
    let mut needs_component_is_iife_close = false;

    // Convert tag for components
    match el.tag_type {
        TagType::Component => {
            // Handle `<component is="...">` / `<component :is="...">`.
            // Dynamic `:is` wraps the element in an IIFE — needs closing after element end.
            if tag_name == "component"
                && rewrite_component_is(el, oxc_el, ctx.source, ctx.out, ctx.resolver)
            {
                needs_component_is_iife_close = true;
            }
        }
        TagType::Template => {
            // Named slot: preserve slot name at original position for intellisense.
            // <template #header> → <>{"header"}
            // <template v-slot:header> → <>{"header"}
            let is_self_closing = el.tag_close.is_none();
            // Self-closing templates need <></> since there's no closing tag to rewrite
            let frag_suffix = if is_self_closing { "</>" } else { "" };
            if let Some(ref v_slot) = el.v_slot {
                if let (Some(arg_start), Some(arg_end)) = (v_slot.arg_start, v_slot.arg_end) {
                    // Overwrite everything before slot name → <>{"
                    ctx.out.overwrite(el.tag_open.start, arg_start, "<>{\"");
                    // Slot name stays at [arg_start, arg_end) — sourcemap preserves position
                    // Overwrite everything after slot name through close of open tag → "}
                    ctx.out
                        .overwrite(arg_end, el.tag_open.end, &format!("\"}}{frag_suffix}"));
                } else {
                    // Default slot (no name): <template v-slot> → <>
                    ctx.out.overwrite(
                        el.tag_open.start,
                        el.tag_open.end,
                        &format!("<>{frag_suffix}"),
                    );
                }
            } else {
                // Plain <template> wrapper → <>
                ctx.out.overwrite(
                    el.tag_open.start,
                    el.tag_open.end,
                    &format!("<>{frag_suffix}"),
                );
            }
        }
        TagType::SlotOutlet => {
            // <slot name="x" :prop="val">fallback</slot>
            // → {___VERTER___instance.$slots.x?.({ prop: val }) ?? <>fallback</>}
            //
            // Uses fine-grained overwrites (not overwrite-all + prepends) so that
            // vue_to_tsx interpolation stays bounded. Each overwrite creates a source
            // map boundary, preventing positions within the tag from interpolating
            // past `$slots.name` into the `?.()` call site (which caused `() any` hover).
            let has_children = el.content.as_ref().is_some_and(|c| !c.children.is_empty());

            // Extract slot name + source positions from props
            let slot_info = extract_slot_name(el, ctx.source);

            // Collect slot props (non-name, non-structural attributes)
            let slot_props = collect_slot_props(el, oxc_el, ctx.source, ctx.resolver);

            // Build the call suffix: `?.()` or `?.({ props })`, with `}` or `?? <>`
            // Inside v-for, omit the closing `}` since we don't emit the opening `{`
            // (to avoid `=> ({...})` being parsed as parenthesized object literal).
            let jsx_close = if has_v_for { "" } else { "}" };
            let call_suffix = if slot_props.is_empty() {
                if has_children {
                    "?.() ?? <>".to_string()
                } else {
                    format!("?.(){jsx_close}")
                }
            } else if has_children {
                format!("?.({{ {} }}) ?? <>", slot_props)
            } else {
                format!("?.({{ {} }}){jsx_close}", slot_props)
            };

            // Fine-grained overwrites for source map accuracy:
            // 1. `<` → `{___VERTER___instance.` (or no `{` inside v-for to avoid
            //    `=> ({...})` being parsed as parenthesized object literal)
            let slot_prefix = if has_v_for {
                "___VERTER___instance."
            } else {
                "{___VERTER___instance."
            };
            ctx.out
                .overwrite(el.tag_open.start, el.tag_open.start + 1, slot_prefix);
            // 2. `slot` → `$slots`
            ctx.out
                .overwrite(el.tag_open.start + 1, el.tag_open.name_end, "$slots");

            if let Some(ref info) = slot_info {
                // Static name: overwrite gap between tag name and value to `.`,
                // keep the name value in place, overwrite rest to call suffix.
                if is_valid_js_ident(info.name) {
                    // Dot notation: ` name="` → `.`
                    ctx.out
                        .overwrite(el.tag_open.name_end, info.value_start, ".");
                    // Keep slot name value (source mapped)
                    // `" />` or `" >` → call suffix
                    ctx.out
                        .overwrite(info.value_end, el.tag_open.end, &call_suffix);
                } else {
                    // Bracket notation for non-ident names (e.g., `overlay-content`):
                    // ` name="` → `['`
                    ctx.out
                        .overwrite(el.tag_open.name_end, info.value_start, "['");
                    // Keep slot name value (source mapped)
                    // `" />` → `']` + call suffix
                    ctx.out.overwrite(
                        info.value_end,
                        el.tag_open.end,
                        &format!("']{}", call_suffix),
                    );
                }
            } else {
                // No static name (default slot or dynamic :name):
                // overwrite everything after tag name to `.default` + call suffix
                ctx.out.overwrite(
                    el.tag_open.name_end,
                    el.tag_open.end,
                    &format!(".default{}", call_suffix),
                );
            }

            // Close tag
            if let Some(tag_close) = &el.tag_close {
                if has_children {
                    let close_suffix = if has_v_for { "</>" } else { "</>}" };
                    ctx.out
                        .overwrite(tag_close.start, tag_close.end, close_suffix);
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

    // Void HTML elements (<br>, <input>, <img>, <hr>, etc.) need self-closing in JSX.
    // The parser sets is_self_closing for void tags, but the source may lack the `/`.
    // Check if the source `>` at tag_open.end-1 is preceded by `/` — if not, add it.
    if el.tag_close.is_none() && el.content.is_none() {
        let end_byte = el.tag_open.end as usize;
        if end_byte >= 2
            && ctx.source.as_bytes().get(end_byte - 1) == Some(&b'>')
            && ctx.source.as_bytes().get(end_byte - 2) != Some(&b'/')
        {
            ctx.out
                .overwrite(el.tag_open.end - 1, el.tag_open.end, " />");
        }
    }

    // Walk children — children inherit the condition scopes from this element
    if let Some(content) = &el.content {
        walk_children_with_iife_tracking(&content.children, ctx, &full_scopes);
    }

    // Fix closing tag case mismatch: Vue is case-insensitive for closing tags
    // (e.g., <Button>...</button>) but JSX requires exact case match. Rewrite the
    // closing tag name to match the opening tag when they differ.
    if let Some(tag_close) = &el.tag_close {
        let open_name = &ctx.source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let close_name = &ctx.source[tag_close.start as usize + 2..tag_close.name_end as usize];
        if open_name != close_name && open_name.eq_ignore_ascii_case(close_name) {
            ctx.out
                .overwrite(tag_close.start + 2, tag_close.name_end, open_name);
        }
    }

    // Handle close tag for <template> → </>
    if el.tag_type == TagType::Template {
        if let Some(tag_close) = &el.tag_close {
            ctx.out.overwrite(tag_close.start, tag_close.end, "</>");
        }
    }

    // Close dynamic <component :is> IIFE wrapper — must be before v-if/v-for close
    // so the IIFE is innermost: `{(() => { ...; return <comp/>; })()}`
    if needs_component_is_iife_close {
        let el_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);
        let iife_close = if has_v_for { "; })()" } else { "; })()}" };
        ctx.out.prepend_alloc(el_end, iife_close);
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
            // <template v-if v-slot> — v-if is handled by slot codegen, not IIFE
            let is_slot_template = child_el.tag_type == TagType::Template
                && child_el.v_condition.is_some()
                && child_el.v_slot.is_some();
            if !uses_ternary && !is_slot_template {
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
                // v-if + v-for uses ternary OR slot template — flush pending
                if let Some(pos) = pending_iife_close_pos.take() {
                    ctx.out.prepend_alloc(pos, "}}");
                }
            }
        } else {
            // Text/comment/interpolation — flush pending, unless the next
            // non-whitespace sibling continues the v-if chain (v-else-if/v-else)
            let chain_continues = pending_iife_close_pos.is_some()
                && next_sibling_continues_v_if_chain(children, idx, ctx.ast, ctx.source);
            if pending_iife_close_pos.is_some() && !chain_continues {
                if let Some(pos) = pending_iife_close_pos.take() {
                    ctx.out.prepend_alloc(pos, "}}");
                }
            }

            // Suppress comments between v-if/v-else-if/v-else siblings.
            // JSX comments ({/* */}) between `}` and `else{` break the JS control flow.
            if chain_continues {
                if let AstNodeKind::Comment(c) = &child_node.kind {
                    ctx.out.overwrite(c.start, c.end, "");
                    continue;
                }
            }
        }

        walk_node(child_id, ctx, parent_condition_scopes);

        // After walking a v-if element, inject repositioned comments inside the IIFE.
        // Skip when the element also has a dynamic `<component :is>` — that generates
        // a nested IIFE with `return`, and a JSX comment between `return` and the element
        // would be parsed as `return {}` (object literal), breaking the syntax.
        if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
            let has_dynamic_is = child_el.tag_type == TagType::Component
                && &ctx.source
                    [child_el.tag_open.start as usize + 1..child_el.tag_open.name_end as usize]
                    == "component"
                && child_el.props.iter().any(|p| {
                    p.is_directive
                        && directive_name(p, ctx.source) == "bind"
                        && p.arg_start
                            .zip(p.arg_end)
                            .map(|(a, b)| &ctx.source[a as usize..b as usize] == "is")
                            .unwrap_or(false)
                });
            if matches!(
                child_el.v_condition.as_ref().map(|c| &c.kind),
                Some(ElementNodeConditionKind::If)
            ) && !comment_reposition_set.is_empty()
                && !has_dynamic_is
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
        // Skip for elements that don't emit IIFE wrapping:
        // - v-if + v-for: uses ternary instead of IIFE
        // - <template v-if v-slot>: v-if handled by slot codegen, no IIFE
        if let AstNodeKind::Element(child_el) = &ctx.ast.nodes[child_id.0].kind {
            let uses_ternary = child_el.v_for.is_some()
                && child_el
                    .v_condition
                    .as_ref()
                    .is_some_and(|c| c.kind == ElementNodeConditionKind::If);
            let is_slot_template = child_el.tag_type == TagType::Template
                && child_el.v_condition.is_some()
                && child_el.v_slot.is_some();
            if !uses_ternary && !is_slot_template {
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

/// Rewrite `<component :is="expr">` to use `extractRenderComponent`.
/// Returns `true` if the dynamic `:is` pattern was used (requires IIFE close after element).
fn rewrite_component_is<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) -> bool {
    let static_is_prop = el.props.iter().find(|prop| {
        if prop.is_directive {
            return false;
        }
        &source[prop.start as usize..prop.name_end as usize] == "is"
    });

    // 1) Static `is="div"`
    if let Some(is_prop) = static_is_prop {
        let (Some(value_start), Some(value_end)) = (is_prop.value_start, is_prop.value_end) else {
            return false;
        };
        if value_end <= value_start {
            return false;
        }

        let target_tag = source[value_start as usize..value_end as usize].trim();
        if target_tag.is_empty() {
            return false;
        }

        rewrite_component_tag_name(el, target_tag, out);

        // Remove `is="..."`
        let is_prop_end = props::get_prop_end(is_prop);
        out.overwrite(is_prop.start, is_prop_end, "");
        return false;
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
        return false;
    };

    let (Some(value_start), Some(value_end)) = (bind_is_prop.value_start, bind_is_prop.value_end)
    else {
        return false;
    };
    if value_end <= value_start {
        return false;
    }

    let value_expr = source[value_start as usize..value_end as usize].trim();
    if value_expr.is_empty() {
        return false;
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

    // Wrap in IIFE so the `const` declaration is valid in any context.
    // In JSX children: {(() => { const comp = ...; return <comp/>; })()}
    // In v-for body:    (() => { const comp = ...; return <comp/>; })()
    // The outer {} is JSX expression syntax — needed in JSX but causes
    // a parse error in v-for's `=> (...)` expression context (parsed as object literal).
    let temp_name = "___VERTER___component_render";
    let in_v_for = el.v_for.is_some();
    let iife_prefix = if in_v_for {
        format!(
            "(() => {{ const {}=___VERTER___extractRenderComponent(",
            temp_name
        )
    } else {
        format!(
            "{{(() => {{ const {}=___VERTER___extractRenderComponent(",
            temp_name
        )
    };
    let content = format!("{}{}); return ", iife_prefix, resolved_expr);
    // Use mapped emission so the expression gets a source map token.
    // This allows TSGO to map hover positions back to the Vue template.
    out.prepend_alloc_mapped_with_offset(
        el.tag_open.start,
        value_start,
        iife_prefix.len() as u32,
        &content,
    );
    rewrite_component_tag_name(el, temp_name, out);

    // Remove `:is="..."`
    let prop_end = props::get_prop_end(bind_is_prop);
    out.overwrite(bind_is_prop.start, prop_end, "");
    true
}

fn rewrite_component_tag_name(el: &ElementNode, target_tag: &str, out: &mut CodeGenOutput<'_>) {
    // Rewrite opening `<component` to `<targetTag`.
    out.overwrite(el.tag_open.start + 1, el.tag_open.name_end, target_tag);

    // Rewrite closing `</component>` (or `</component :is="as">`) if present.
    // Use `end - 1` instead of `name_end` to strip any trailing attributes on
    // the closing tag (e.g., `</component :is="as">` is technically valid HTML
    // but produces invalid JSX if the attributes are preserved).
    if let Some(tag_close) = &el.tag_close {
        out.overwrite(tag_close.start + 2, tag_close.end - 1, target_tag);
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

/// Check if a string is a valid JS identifier (can be used as a bare property name).
fn is_valid_js_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Extracted slot name info for fine-grained source map overwrites.
struct SlotNameInfo<'a> {
    /// The slot name string (e.g., "header")
    name: &'a str,
    /// Source position of the name value start (inside quotes)
    value_start: u32,
    /// Source position of the name value end (before closing quote)
    value_end: u32,
}

/// Extract the slot name from a `<slot>` element's attributes.
/// Returns the name string and its source positions for sourcemap mapping.
///
/// - `<slot>` → `None` (will use "default")
/// - `<slot name="header">` → `Some(SlotNameInfo { name: "header", value_start, value_end })`
/// - `<slot :name="dynamicName">` → `None` (dynamic, falls back to "default")
fn extract_slot_name<'a>(el: &ElementNode, source: &'a str) -> Option<SlotNameInfo<'a>> {
    for prop in &el.props {
        if !prop.is_directive {
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let name = source[vs as usize..ve as usize].trim();
                    if !name.is_empty() {
                        return Some(SlotNameInfo {
                            name,
                            value_start: vs,
                            value_end: ve,
                        });
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
            let key = quote_prop_key_if_needed(attr_name);
            // Static attribute: name="value"
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                parts.push(format!("{}: \"{}\"", key, value));
            } else {
                parts.push(format!("{}: true", key));
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
                        let key = quote_prop_key_if_needed(arg);
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
                            parts.push(format!("{}: {}", key, resolved));
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

/// Quote a property key if it's not a valid JS identifier (e.g., contains hyphens).
/// `item-class` → `"item-class"`, `itemClass` → `itemClass` (unchanged).
fn quote_prop_key_if_needed(key: &str) -> String {
    if is_valid_js_ident(key) {
        key.to_string()
    } else {
        format!("\"{}\"", key)
    }
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

    // Escape characters that are invalid inside a `"..."` JS string literal:
    // - double quotes → \"
    // - newlines → \n (multi-line strings are illegal in JS)
    // - carriage returns → \r
    // - backslashes → \\ (must escape first to avoid double-escaping)
    for (i, b) in trimmed.as_bytes().iter().enumerate() {
        let pos = content_start + i as u32;
        match *b {
            b'\\' => out.overwrite(pos, pos + 1, "\\\\"),
            b'"' => out.overwrite(pos, pos + 1, "\\\""),
            b'\n' => out.overwrite(pos, pos + 1, "\\n"),
            b'\r' => out.overwrite(pos, pos + 1, "\\r"),
            _ => {}
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
mod tests;
