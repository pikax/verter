//! # verter_ffi — Shared FFI types and conversions
//!
//! Single source of truth for all types and conversion logic shared between
//! the NAPI (`verter_napi`) and WASM (`verter_wasm`) binding crates.
//!
//! ## Why a separate crate?
//!
//! The [`verter_host`] types cannot be directly serialized to JavaScript because
//! they use Rust enums with associated data (`VirtualNodeKind::Style { index }`),
//! `Arc<str>` fields, `usize` indices, and Rust enum variants as strings. This
//! crate provides the flat, serde-compatible FFI types and the conversion
//! functions between them and host types.
//!
//! ## Architecture
//!
//! - **`types`** — All FFI structs with `#[serde(rename_all = "camelCase")]`.
//!   WASM uses these directly via `serde_wasm_bindgen`. NAPI maps to/from
//!   its own `#[napi(object)]` structs via zero-copy `From` impls.
//!
//! - **`convert`** — Framework-agnostic conversion functions between FFI types
//!   and [`verter_host`] types. Errors use [`convert::FfiConversionError`] —
//!   each consumer converts via the `Display` impl to its native error type.

pub mod convert;
pub mod types;
