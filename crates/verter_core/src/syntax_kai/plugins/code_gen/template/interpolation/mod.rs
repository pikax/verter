use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType, plugins::code_gen::template::helper::patch_bindings,
        types::OxcInterpolation,
    },
};

pub fn handle_interpolation<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    ev: &OxcInterpolation<'alloc>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
) {
    // Example: transform {{ msg }} to {{ msg.toUpperCase() }}
    // ctx.code_transform.append_right(ev.expression.end, ".toUpperCase()");
    patch_bindings(code_transform, &ev.bindings, map, is_inline);

    // convert {{ to (
    code_transform.overwrite(ev.start, ev.content.start, "(");

    // convert }} to )
    code_transform.overwrite(ev.content.end, ev.end, ")");

    code_transform.prepend_left(ev.start, "_toDisplayString");
}
