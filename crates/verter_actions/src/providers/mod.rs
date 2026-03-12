//! Built-in action providers.

mod add_component_is;
mod extract_bare_text;
mod html_self_close;
mod insert_attribute;
mod insert_type_param;
mod prefer_script_attrs;
mod remove_attribute;
mod remove_directive;
mod remove_import;
mod remove_inline_template_attr;
mod remove_stray_directive;
mod remove_template_attr;
mod remove_unsafe_url;
mod remove_unused_css;
mod rename_casing;
mod replace_content;
mod replace_directive;
mod shorthand_directive;
mod ssr_wrap;
mod symbol_provide;
mod toggle_negation;
mod unwrap_binding;
mod v_bind_shorthand;

pub use add_component_is::AddComponentIs;
pub use extract_bare_text::ExtractBareText;
pub use html_self_close::HtmlSelfClose;
pub use insert_attribute::InsertAttribute;
pub use insert_type_param::InsertTypeParam;
pub use prefer_script_attrs::PreferScriptAttrs;
pub use remove_attribute::RemoveAttribute;
pub use remove_directive::RemoveDirective;
pub use remove_import::RemoveImport;
pub use remove_inline_template_attr::RemoveInlineTemplateAttr;
pub use remove_stray_directive::RemoveStrayDirective;
pub use remove_template_attr::RemoveTemplateAttr;
pub use remove_unsafe_url::RemoveUnsafeUrl;
pub use remove_unused_css::RemoveUnusedCss;
pub use rename_casing::RenameCasing;
pub use replace_content::ReplaceContent;
pub use replace_directive::ReplaceDirective;
pub use shorthand_directive::ShorthandDirective;
pub use ssr_wrap::SsrWrap;
pub use symbol_provide::SymbolProvide;
pub use toggle_negation::ToggleNegation;
pub use unwrap_binding::UnwrapBinding;
pub use v_bind_shorthand::VBindShorthand;

use crate::engine::ActionEngine;

/// Register all built-in providers on the engine.
pub fn register_builtin_providers(engine: &mut ActionEngine) {
    engine.register(Box::new(RemoveUnusedCss));
    engine.register(Box::new(RemoveDirective));
    engine.register(Box::new(RemoveImport));
    engine.register(Box::new(RemoveTemplateAttr));
    engine.register(Box::new(RemoveStrayDirective));
    engine.register(Box::new(AddComponentIs));
    engine.register(Box::new(RemoveInlineTemplateAttr));
    engine.register(Box::new(HtmlSelfClose));
    engine.register(Box::new(ShorthandDirective));
    engine.register(Box::new(SymbolProvide));
    engine.register(Box::new(RemoveUnsafeUrl));
    engine.register(Box::new(VBindShorthand));
    engine.register(Box::new(InsertAttribute));
    engine.register(Box::new(ReplaceDirective));
    engine.register(Box::new(UnwrapBinding));
    engine.register(Box::new(RemoveAttribute));
    engine.register(Box::new(ToggleNegation));
    engine.register(Box::new(ReplaceContent));
    engine.register(Box::new(RenameCasing));
    engine.register(Box::new(PreferScriptAttrs));
    engine.register(Box::new(ExtractBareText));
    engine.register(Box::new(SsrWrap));
    engine.register(Box::new(InsertTypeParam));
}
