use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform, syntax_kai::binding_types::BindingType,
    utils::oxc::BindingExtractionResult,
};

pub fn patch_bindings<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    bindings: &Option<BindingExtractionResult<'alloc>>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
) {
    if let Some(bindings) = bindings {
        bindings.bindings.iter().for_each(|f| {
            if !f.ignore {
                let b = &map[&f.name];
                code_transform.prepend_left(f.span.start, b.accessor_prefix(is_inline));
            }
        });
    }
}
