//! SSR safety and best practice lint rules.
//!
//! These rules detect patterns that are problematic in server-side rendering:
//! client-only APIs in setup scope, hydration mismatches, and missing SSR
//! best practices. All rules are gated behind `config.ssr_mode`.

mod no_browser_globals_in_setup;
mod no_client_only_lifecycle_in_setup;
mod no_css_var_manipulation_in_setup;
mod no_dom_query_in_setup;
mod no_nondeterministic_in_template;
mod no_side_effects_in_setup_for_ssr;
mod no_template_ref_in_setup;
mod no_v_show_prefer_v_if;
mod prefer_server_prefetch;
mod require_client_only_wrapper;

pub use no_browser_globals_in_setup::NoBrowserGlobalsInSetup;
pub use no_client_only_lifecycle_in_setup::NoClientOnlyLifecycleInSetup;
pub use no_css_var_manipulation_in_setup::NoCssVarManipulationInSetup;
pub use no_dom_query_in_setup::NoDomQueryInSetup;
pub use no_nondeterministic_in_template::NoNondeterministicInTemplate;
pub use no_side_effects_in_setup_for_ssr::NoSideEffectsInSetupForSsr;
pub use no_template_ref_in_setup::NoTemplateRefInSetup;
pub use no_v_show_prefer_v_if::NoVShowPreferVIf;
pub use prefer_server_prefetch::PreferServerPrefetch;
pub use require_client_only_wrapper::RequireClientOnlyWrapper;
