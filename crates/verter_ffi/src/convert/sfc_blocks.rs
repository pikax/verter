//! SFC block (template / script / style / custom) and attribute conversions
//! from semantic-analysis types to FFI types.

use crate::types::*;

pub(super) fn sfc_attribute_to_ffi(
    attribute: verter_semantic::analysis::component_meta::SfcAttributeAnalysis,
) -> FfiSfcAttributeMeta {
    FfiSfcAttributeMeta {
        name: attribute.name,
        value: attribute.value,
    }
}

pub(super) fn template_block_to_ffi(
    block: verter_semantic::analysis::component_meta::TemplateBlockAnalysis,
) -> FfiTemplateBlockMeta {
    FfiTemplateBlockMeta {
        lang: block.lang,
        src: block.src,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

pub(super) fn script_block_to_ffi(
    block: verter_semantic::analysis::component_meta::ScriptBlockAnalysis,
) -> FfiScriptBlockMeta {
    FfiScriptBlockMeta {
        lang: block.lang,
        src: block.src,
        generic: block.generic,
        attrs_type: block.attrs_type,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

pub(super) fn style_block_to_ffi(
    block: verter_semantic::analysis::component_meta::StyleBlockInfoAnalysis,
) -> FfiStyleBlockMeta {
    FfiStyleBlockMeta {
        index: block.index as u32,
        lang: block.lang,
        src: block.src,
        scoped: block.scoped,
        is_module: block.is_module,
        module_name: block.module_name,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}

pub(super) fn custom_block_to_ffi(
    block: verter_semantic::analysis::component_meta::CustomBlockAnalysis,
) -> FfiCustomBlockMeta {
    FfiCustomBlockMeta {
        index: block.index as u32,
        block_type: block.block_type,
        lang: block.lang,
        src: block.src,
        attributes: block
            .attributes
            .into_iter()
            .map(sfc_attribute_to_ffi)
            .collect(),
    }
}
