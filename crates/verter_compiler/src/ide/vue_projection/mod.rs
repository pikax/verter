//! Vue projection products consumed by the dormant projection backend.
//!
//! Vue IDE routing stays on [`super::script`] until atomic activation.

pub mod binder_capture;
pub mod options_api;
pub mod script_setup;

#[cfg(test)]
mod binder_capture_tests;
#[cfg(test)]
mod options_api_tests;
#[cfg(test)]
mod script_setup_tests;
