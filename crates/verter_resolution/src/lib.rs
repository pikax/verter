//! Reusable, I/O-free implementation of workspace module-resolution authority.
//! Inputs and answer vocabulary belong to `verter_session_query::resolution`.

#![forbid(unsafe_code)]

pub mod attempt_outcome;
pub mod attempt_output;
pub mod input_resolution_budgets;
mod module_reference_resolution;
mod module_resolver_core;
mod node_modules_resolution;
pub mod observation;
mod package_target_resolution;
pub mod path_utils;
mod preferred_specifier_resolution;
mod priority_frontier;
mod probe_path_resolution;
mod project_ownership_resolution;
mod project_references_resolution;
mod provider_projection_resolution;
mod resolve_frame;
pub mod resolver_attempt_view;
mod source_id_resolution;
mod top_level_resolution;
mod tsconfig_paths_resolution;

pub use attempt_outcome::{CompletedAttempt, KernelAttempt};
pub use attempt_output::AttemptOutput;
pub use input_resolution_budgets::InputResolutionRetention;
pub use module_reference_resolution::{
    collect_resolvable_module_reference_specifiers, resolve_known_module_reference_dependencies,
};
pub use module_resolver_core::ModuleResolverCore;
pub use node_modules_resolution::{ancestor_dirs, ancestor_dirs_from_dir};
pub use observation::ResolverObservation;
pub use path_utils::resolve_known_dependency_id;
pub use resolve_frame::ResolveFrame;
pub use resolver_attempt_view::ResolverAttemptView;

#[must_use]
pub fn probe_extensions() -> &'static [&'static str] {
    probe_path_resolution::probe_extensions_list()
}
