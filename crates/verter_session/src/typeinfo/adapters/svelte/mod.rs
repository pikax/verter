#![deny(missing_docs)]
//! The Svelte typeinfo adapter scaffolding.
//!
//! The Svelte SURFACE adapter (the plan/normalize half that maps runes
//! semantics onto the six wire surface kinds) is a later vertical (B8b) — the
//! Svelte carrier registers with a `SurfaceRegistration::Deferred` arm until it
//! lands. This module currently owns ONLY the blessed parse-carrier accessor:
//!
//! - [`parse_access`] — the `svelte_parse()` carrier accessor + the
//!   carrier-token receipt the registry's Svelte carrier leg reuses.

pub mod parse_access;

pub(crate) use parse_access::{svelte_carrier_token_clone, svelte_parse};
