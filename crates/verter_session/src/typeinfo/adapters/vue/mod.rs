#![deny(missing_docs)]
//! The Vue SFC typeinfo adapter — the plan/normalize half.
//!
//! Turns a `.vue` single-file component into the shared typeinfo substrate's
//! value types. The Vue RESOLUTION machinery (the public component type and the
//! FullMetadata macro surface + its normalizers) lives executor-side in
//! [`crate::typeinfo::framework_surface::vue_exec`]; the `impl VerterHost` entry
//! points there stay the public API current consumers call. This module owns the
//! framework-adapter half:
//!
//! - [`adapter`] — [`adapter::VueFrameworkAdapter`], the registry's Vue
//!   [`FrameworkSurfaceAdapter`](crate::typeinfo::framework_surface::FrameworkSurfaceAdapter):
//!   it PLANS the Vue component's typed surface demands and NORMALIZES the
//!   executor-resolved surfaces into per-kind DTO bundles. It holds NO
//!   resolution.
//! - [`parse_access`] — the `vue_parse()` carrier accessor + the carrier-token
//!   receipt.

pub mod adapter;
pub mod parse_access;

pub(crate) use parse_access::{receive_vue_carrier_token, vue_carrier_token_clone, vue_parse};
