#![deny(missing_docs)]
//! Per-adapter [`ComponentApiProjector`](super::api_projector::ComponentApiProjector)
//! legs.

pub mod vue;

pub use vue::VueComponentApiProjector;
