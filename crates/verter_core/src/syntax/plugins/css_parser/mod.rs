#[allow(clippy::module_inception)]
pub mod css_parser;
pub mod transformer;

pub use css_parser::CssParserPlugin;
pub use transformer::{
    transform_css_modules, transform_scoped_css, ModulesTransformResult, TransformResult,
};
