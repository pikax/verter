//! Script analysis lint rules.

mod define_macros_order;
mod no_async_in_computed;
mod no_inline_lifecycle;
mod no_lifecycle_after_await;
mod no_unused_emit_declarations;
mod no_watch_after_await;
mod prefer_use_template_ref;
mod require_default_prop;
mod require_symbol_provide;

pub use define_macros_order::DefineMacrosOrder;
pub use no_async_in_computed::NoAsyncInComputed;
pub use no_inline_lifecycle::NoInlineLifecycle;
pub use no_lifecycle_after_await::NoLifecycleAfterAwait;
pub use no_unused_emit_declarations::NoUnusedEmitDeclarations;
pub use no_watch_after_await::NoWatchAfterAwait;
pub use prefer_use_template_ref::PreferUseTemplateRef;
pub use require_default_prop::RequireDefaultProp;
pub use require_symbol_provide::RequireSymbolProvide;
