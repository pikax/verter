#![deny(missing_docs)]
//! The Svelte typeinfo adapter.
//!
//! - [`adapter`] — the [`SvelteFrameworkAdapter`](adapter::SvelteFrameworkAdapter)
//!   plan/normalize half that maps runes/legacy semantics onto the six wire
//!   surface kinds (the executor owns the per-source resolution leg in
//!   `framework_surface::svelte_exec`).
//! - [`parse_access`] — the `svelte_parse()` carrier accessor + the
//!   carrier-token receipt the registry's Svelte carrier leg reuses.

pub mod adapter;
pub mod parse_access;

pub(crate) use parse_access::{svelte_carrier_token_clone, svelte_parse};
