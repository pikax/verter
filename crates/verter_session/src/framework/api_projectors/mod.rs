#![deny(missing_docs)]
//! Per-adapter [`ComponentApiProjector`](super::api_projector::ComponentApiProjector)
//! legs.

pub mod svelte;
pub mod vue;

pub use svelte::SvelteComponentApiProjector;
pub use vue::VueComponentApiProjector;
