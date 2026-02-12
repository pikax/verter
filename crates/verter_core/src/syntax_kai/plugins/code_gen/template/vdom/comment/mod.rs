use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugin::SyntaxPluginContext, plugins::code_gen::types::TemplateImportDependencies,
        types::Comment,
    },
};

use super::{ChildInfo, ChildKind, StateStack};

/// Process a comment node within a template element.
///
/// Transforms `<!-- content -->` into `_createCommentVNode("content")`.
/// Records a `ChildInfo` in the parent state. Does NOT add separators —
/// the close phase retroactively inserts separators.
pub(crate) fn handle_comment<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    ev: &Comment,
    _ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack,
    imports: &mut TemplateImportDependencies,
) {
    state.children.push(ChildInfo {
        start: ev.start,
        kind: ChildKind::Comment,
        scope_prefix: String::new(),
    });

    imports.add(TemplateImportDependencies::CREATE_COMMENT_VNODE);

    // Replace `<!--` region with `_createCommentVNode("`
    code_transform.overwrite(ev.start, ev.content.start, "_createCommentVNode(\"");

    // Replace `-->` region with `")`
    code_transform.overwrite(ev.content.end, ev.end, "\")");
}
