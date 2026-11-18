//! Vue projection products consumed by the dormant projection backend.
//!
//! Vue IDE routing stays on [`super::script`] until atomic activation.

pub mod attribute_operations;
pub mod binder_capture;
pub mod binding_views;
pub mod component_resolution;
pub mod component_use;
pub mod event_keys;
pub mod generic_interop;
pub mod options_api;
pub mod props;
pub mod public_constructor;
pub mod script_setup;

// The checking module and the template context it reads.
pub mod checking_fragment;
pub mod checking_module;
pub(crate) mod composed_product;
pub(crate) mod js_dialect;
pub mod template_context;

// Product modules.
pub mod branches;
pub mod builtins;
pub mod callback_reentry;
pub mod declarations;
pub mod directives;
pub mod event_payloads;
pub mod external_sources;
pub mod fallthrough;
pub mod iteration;
pub mod models;
pub mod native_elements;
pub mod native_models;
pub mod provided_slots;
pub mod public_instance;
pub mod slot_outlets;

// Callable adaptation, feature closure, recovery, strict spreads and styles.
pub mod callable_adapter;
pub mod feature_closure;
pub mod recovery;
pub mod strict_spread;
pub mod style_modules;
pub mod style_vbind;

#[cfg(test)]
mod attribute_operations_tests;
#[cfg(test)]
mod binder_capture_tests;
#[cfg(test)]
mod binding_views_tests;
#[cfg(test)]
mod component_resolution_tests;
#[cfg(test)]
mod component_use_tests;
#[cfg(test)]
mod event_keys_tests;
#[cfg(test)]
mod generic_interop_tests;
#[cfg(test)]
mod options_api_tests;
#[cfg(test)]
mod props_tests;
#[cfg(test)]
mod public_constructor_tests;
#[cfg(test)]
mod script_setup_tests;

// Unit tests of the checking module, its products and the reserved homes.
#[cfg(test)]
mod branches_tests;
#[cfg(test)]
mod builtins_tests;
#[cfg(test)]
mod callable_adapter_tests;
#[cfg(test)]
mod callback_reentry_tests;
#[cfg(test)]
mod checking_fragment_tests;
#[cfg(test)]
mod checking_module_tests;
#[cfg(test)]
mod declarations_tests;
#[cfg(test)]
mod directives_tests;
#[cfg(test)]
mod event_payloads_tests;
#[cfg(test)]
mod external_sources_tests;
#[cfg(test)]
mod fallthrough_tests;
#[cfg(test)]
mod feature_closure_tests;
#[cfg(test)]
mod iteration_tests;
#[cfg(test)]
mod js_dialect_tests;
#[cfg(test)]
mod models_tests;
#[cfg(test)]
mod native_elements_tests;
#[cfg(test)]
mod native_models_tests;
#[cfg(test)]
mod provided_slots_tests;
#[cfg(test)]
mod public_instance_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod slot_outlets_tests;
#[cfg(test)]
mod strict_spread_tests;
#[cfg(test)]
mod style_modules_tests;
#[cfg(test)]
mod style_vbind_tests;
#[cfg(test)]
mod template_context_tests;
