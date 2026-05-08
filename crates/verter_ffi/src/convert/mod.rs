//! Framework-agnostic conversion functions between FFI types and host types.
//!
//! Error-returning functions use `Result<T, FfiConversionError>`. Each consumer
//! crate converts the error to its native type (`napi::Error` or `JsValue`)
//! via the `Display` impl.
//!
//! ## Module organization
//!
//! - [`error`] — `FfiConversionError` and its `Display` / `Error` / `From` impls.
//! - [`string_helpers`] — small enum-to-string and per-field shared helpers.
//! - [`sfc_blocks`] — template / script / style / custom block conversions.
//! - [`fallthrough`] — root-reachability and fallthrough surface conversions.
//! - [`component_meta`] — component-meta analysis → FFI projection (public entry points).
//! - [`input`] — FFI → host input conversions (config, profile, upserts, queries).
//! - [`output`] — host → FFI output conversions (diagnostics, updates, errors).
//! - [`actions`] — `verter_actions::CodeAction` → FFI projection.
//! - [`lint`] — lint diagnostics span conversion and lint-rule metadata projection.
//! - [`offset`] — UTF-8 ↔ UTF-16 / UTF-32 offset conversions and destructured-binding
//!   span translation.

mod actions;
mod component_meta;
pub mod error;
mod fallthrough;
mod input;
mod lint;
mod offset;
mod output;
mod sfc_blocks;
mod string_helpers;
mod typeinfo;

#[cfg(test)]
mod tests;

pub use actions::*;
pub use component_meta::*;
pub use error::*;
pub use input::*;
pub use lint::*;
pub use offset::*;
pub use output::*;
pub use typeinfo::*;
