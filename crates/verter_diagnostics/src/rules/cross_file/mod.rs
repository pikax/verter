//! Cross-file lint rules.

mod deep_composable_tracking;
mod no_duplicate_vue;
mod provide_inject_validation;

pub use deep_composable_tracking::DeepComposableTracking;
pub use no_duplicate_vue::NoDuplicateVue;
pub use provide_inject_validation::ProvideInjectValidation;
