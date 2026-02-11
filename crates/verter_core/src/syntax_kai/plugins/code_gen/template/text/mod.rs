use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugin::SyntaxPluginContext, plugins::code_gen::types::TemplateImportDependencies,
        types::Text,
    },
};

use super::{ChildInfo, ChildKind, StateStack};

/// Process a text node within a template element.
///
/// Records a `ChildInfo` in the parent state and wraps the text content in quotes
/// to produce a JS string literal. Does NOT add separators — the close phase
/// retroactively inserts separators based on the full children list.
///
/// For source like `hello` between elements, this produces `"hello"`.
pub(crate) fn handle_text<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    ev: &Text,
    _ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack,
    _imports: &mut TemplateImportDependencies,
) {
    state.children.push(ChildInfo {
        start: ev.start,
        kind: ChildKind::Text,
        scope_prefix: String::new(),
    });
    state.children_count += 1;

    // TODO: handle has_entity (HTML entity decoding) — overwrite with decoded string
    // For now, add only the closing quote. The opening quote is added by the
    // close phase as part of the separator insertion (see ChildKind::content_prefix).
    // This is necessary because prepend_left is FIFO at the same position — if we
    // prepend `"` here and the close phase later prepends `, `, the quote would
    // appear first: `", hello"` instead of `, "hello"`.

    code_transform.append_left(ev.end, "\"");
}
