//! Vapor mode lint rules.
//!
//! These rules are only active when `LintConfig.vapor_mode` is true.

// @ai-generated

mod no_inline_template;
mod no_non_vapor_components;
mod no_suspense;
mod no_vue_lifecycle_events;

pub use no_inline_template::NoInlineTemplate;
pub use no_non_vapor_components::NoNonVaporComponents;
pub use no_suspense::NoSuspense;
pub use no_vue_lifecycle_events::NoVueLifecycleEvents;
