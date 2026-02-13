use crate::syntax_kai::{
    plugin::SyntaxPluginContext, plugins::code_gen::types::TemplateImportDependencies,
    types::Comment,
};

use super::{ChildInfo, ChildKind, StateStack};

/// Process a comment node within a template element.
///
/// Transforms `<!-- content -->` into `_createCommentVNode("content")`.
/// Records a `ChildInfo` in the parent state. Does NOT add separators —
/// the close phase retroactively inserts separators.
pub(crate) fn handle_comment<'alloc>(
    ev: &Comment,
    _ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    imports: &mut TemplateImportDependencies,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
) {
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
