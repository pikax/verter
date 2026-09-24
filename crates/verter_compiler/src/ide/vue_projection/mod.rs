//! Vue projection products consumed by the dormant projection backend.
//!
//! Vue IDE routing stays on [`super::script`] until atomic activation.

pub mod attribute_operations;
pub mod binder_capture;
pub mod binding_views;
pub mod component_use;
pub mod generic_interop;
pub mod options_api;
pub mod public_constructor;
pub mod script_setup;

#[cfg(test)]
mod attribute_operations_tests;
#[cfg(test)]
mod binder_capture_tests;
#[cfg(test)]
mod binding_views_tests;
#[cfg(test)]
mod component_use_tests;
#[cfg(test)]
mod generic_interop_tests;
#[cfg(test)]
mod options_api_tests;
#[cfg(test)]
mod public_constructor_tests;
#[cfg(test)]
mod script_setup_tests;
