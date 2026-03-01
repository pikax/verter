//! Built-in action providers.

mod add_component_is;
mod html_self_close;
mod remove_directive;
mod remove_inline_template_attr;
mod remove_stray_directive;
mod remove_template_attr;
mod remove_unsafe_url;
mod remove_unused_css;
mod shorthand_directive;
mod symbol_provide;

pub use add_component_is::AddComponentIs;
pub use html_self_close::HtmlSelfClose;
pub use remove_directive::RemoveDirective;
pub use remove_inline_template_attr::RemoveInlineTemplateAttr;
pub use remove_stray_directive::RemoveStrayDirective;
pub use remove_template_attr::RemoveTemplateAttr;
pub use remove_unsafe_url::RemoveUnsafeUrl;
pub use remove_unused_css::RemoveUnusedCss;
pub use shorthand_directive::ShorthandDirective;
pub use symbol_provide::SymbolProvide;

use crate::engine::ActionEngine;

/// Register all built-in providers on the engine.
pub fn register_builtin_providers(engine: &mut ActionEngine) {
    engine.register(Box::new(RemoveUnusedCss));
    engine.register(Box::new(RemoveDirective));
    engine.register(Box::new(RemoveTemplateAttr));
    engine.register(Box::new(RemoveStrayDirective));
    engine.register(Box::new(AddComponentIs));
    engine.register(Box::new(RemoveInlineTemplateAttr));
    engine.register(Box::new(HtmlSelfClose));
    engine.register(Box::new(ShorthandDirective));
    engine.register(Box::new(SymbolProvide));
    engine.register(Box::new(RemoveUnsafeUrl));
}
