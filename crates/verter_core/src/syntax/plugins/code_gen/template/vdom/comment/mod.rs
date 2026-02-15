use crate::syntax::{
    plugin::SyntaxPluginContext, plugins::code_gen::types::TemplateImportDependencies,
    types::Comment,
};

use super::{ChildInfo, ChildKind, StateStack};

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
    _ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    imports: &mut TemplateImportDependencies,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
    comments: bool,
) {
    if !comments {
        // Strip the entire comment from output.
        pending_overwrites.push((ev.start, ev.end, ""));
        return;
    }

    state.children.push(ChildInfo {
        start: ev.start,
        kind: ChildKind::Comment,
        scope_prefix: "",
    });

    imports.add(TemplateImportDependencies::CREATE_COMMENT_VNODE);

    // Replace `<!--` region with `_createCommentVNode("`
    pending_overwrites.push((ev.start, ev.content.start, "_createCommentVNode(\""));

    // Replace `-->` region with `")`
    pending_overwrites.push((ev.content.end, ev.end, "\")"));
}
