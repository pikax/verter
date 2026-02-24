//! Performance lint rules — detect patterns that may hurt rendering performance.

mod max_template_depth;
mod prefer_static_class;

// NOTE: no-use-v-if-with-v-for lives in `vue/` as `NoUseVIfWithVFor`.
// It is also a performance concern but is categorized under VueEssential.

pub use max_template_depth::MaxTemplateDepth;
pub use prefer_static_class::PreferStaticClass;
