use crate::syntax::{
    plugin::SyntaxPluginContext, plugins::code_gen::types::TemplateImportDependencies,
    types::Comment,
};

use super::{ChildInfo, ChildKind, StateStack};

/// Returns true if the comment content needs escaping for a JS string literal.
fn needs_js_escape(content: &str) -> bool {
    content
        .bytes()
        .any(|b| b == b'\n' || b == b'\r' || b == b'"' || b == b'\\')
}

/// Process a comment node within a template element.
///
/// When `comments` is true, transforms `<!-- content -->` into
/// `_createCommentVNode("content")` and records a `ChildInfo` in the parent state.
///
/// When `comments` is false, the comment is stripped from the output entirely:
/// the source region is overwritten with empty string and no child is recorded,
/// so the close phase won't emit separators for it.
pub(crate) fn handle_comment<'alloc>(
    ev: &Comment,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    imports: &mut TemplateImportDependencies,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
    alloc_fn: impl FnOnce(&str) -> &'alloc str,
    comments: bool,
) {
    if !comments {
        // Strip the entire comment from output.
        pending_overwrites.push((ev.start, ev.end, ""));
        return;
    }

    state.children.push(ChildInfo {
        start: ev.start,
        end: ev.end, // stored so v-if continuation can blank out interleaved comments
        kind: ChildKind::Comment,
        scope_prefix: "",
        is_named_slot: false,
    });

    imports.add(TemplateImportDependencies::CREATE_COMMENT_VNODE);

    let content = &ctx.input[ev.content.start as usize..ev.content.end as usize];

    if needs_js_escape(content) {
        // Multi-line or content with special chars: build escaped replacement
        // as a single full-span overwrite to avoid unterminated string literals.
        let mut buf = String::with_capacity(content.len() + 24);
        buf.push_str("_createCommentVNode(\"");
        for ch in content.chars() {
            match ch {
                '\n' => buf.push_str("\\n"),
                '\r' => buf.push_str("\\r"),
                '"' => buf.push_str("\\\""),
                '\\' => buf.push_str("\\\\"),
                _ => buf.push(ch),
            }
        }
        buf.push_str("\")");
        let s = alloc_fn(&buf);
        pending_overwrites.push((ev.start, ev.end, s));
    } else {
        // Simple single-line content: use two overwrites leaving content in-place.
        // Replace `<!--` region with `_createCommentVNode("`
        pending_overwrites.push((ev.start, ev.content.start, "_createCommentVNode(\""));
        // Replace `-->` region with `")`
        pending_overwrites.push((ev.content.end, ev.end, "\")"));
    }
}
