//! Analysis plugin module for template scope analysis.

#[allow(clippy::module_inception)]
pub mod analysis;
pub mod context;

pub use analysis::{Analysis, AnalysisConfig};
