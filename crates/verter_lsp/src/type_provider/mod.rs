//! Provider-neutral TypeScript type-provider integration.
//!
//! These modules are shared across every type-provider backend (TSGO, tsserver,
//! extension). They hold the trait surface, protocol DTO re-exports, result
//! merging, auto-import resolution, project synchronization, and the test mock —
//! none of which is specific to any one provider transport. Backend-specific
//! transport and respawn strategy live alongside their provider (`tsgo`,
//! `tsserver`).

pub mod auto_import;
#[cfg(test)]
mod auto_import_tests;
pub mod lazy_managed;
#[cfg(test)]
mod lazy_managed_tests;
pub mod merge;
pub mod mock;
pub mod project_sync;
pub mod protocol;
pub mod specifier_rewrite;
pub mod traits;
