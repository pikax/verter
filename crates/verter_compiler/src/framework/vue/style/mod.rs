#![allow(dead_code, unused_variables)]
//! Vue style semantics: the component scope plan shared by template, style,
//! server and metadata paths, and the per-block style plan.

pub(crate) mod cascade;
pub(crate) mod css_vars;
pub(crate) mod modules;
mod scope_plan;
#[cfg(test)]
mod scope_plan_tests;
pub(crate) mod scoped;
#[cfg(test)]
mod scoped_tests;

pub(crate) use scope_plan::plan_vue_styles_in_scope;
pub use scope_plan::{
    plan_vue_styles, ScopeId, VueComponentScopePlan, VueCssModuleBinding, VueCssVar,
    VueStyleBlockInput, VueStyleBlockPlan, VueStyleOutcome, VueStylePlan,
};
