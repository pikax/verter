//! Code transformation utilities for efficient string manipulation with source map support.
//!
//! The [`CodeTransform`] type allows you to make surgical edits to source code while tracking
//! the original positions for source map generation. Unlike naive string building, it only
//! constructs the final output when explicitly requested via [`CodeTransform::build_string`].
//!
//! # Example
//!
//! ```ignore
//! use verter_compiler::code_transform::{CodeTransform, SourceMapOptions};
//! use oxc_allocator::Allocator;
//!
//! // Create a transformer from source code
//! let allocator = Allocator::default();
//! let mut code = CodeTransform::new("const x = 'old';\nconst y = 'test';", &allocator);
//!
//! // Make edits (these don't build strings, just track changes)
//! code.overwrite(10, 15, "'new'");
//! code.prepend("// Generated\n");
//! code.append("\nexport { x, y };");
//!
//! // Build the final output (only happens here)
//! let output = code.build_string();
//! assert_eq!(output, "// Generated\nconst x = 'new';\nconst y = 'test';\nexport { x, y };");
//!
//! // Generate a source map
//! let map = code.generate_map(
//!     SourceMapOptions::new()
//!         .with_source("input.js")
//!         .with_file("output.js")
//! );
//! ```

mod batch_ops;
mod chain;
mod chunk;
#[allow(clippy::module_inception)] // CodeTransform struct lives in code_transform module
mod code_transform;
mod fallible;
mod mapping_product;
mod segmented;
mod source_map;

pub use chain::SourceMapChainError;
pub use code_transform::CodeTransform;
pub(crate) use code_transform::GeneratedContentMarker;
pub use code_transform::GeneratedSourceRange;
#[cfg(test)]
pub(crate) use code_transform::{
    code_transform_build_string_call_count, code_transform_construction_count,
    reset_code_transform_build_string_call_count, reset_code_transform_construction_count,
};
// The typed refusal surface of the checked (`try_*`) operations, re-exported
// alongside `CodeTransform` as this module's public error type (the inner
// module is private, so this is its only public path).
#[allow(unused_imports)] // consumed by tests and by out-of-module callers
pub use fallible::CodeTransformError;
// The dual-surface mapping product. Geometry's owner is the transform that
// emitted the bytes, so the product is minted here and nowhere else; see
// `mapping_product`'s module doc for the totality and one-to-many contracts.
pub use mapping_product::{
    CarrierClass, CarrierRegion, InsertionAnchor, MappingProduct, ProjectedClass, ProjectedRegion,
    Span,
};
pub use source_map::{advance_generated_position, SourceMapOptions};
// The additive, opt-in segmented-overwrite primitive's plain data carrier.
// `pub` only because it rides inside otherwise-`pub` types elsewhere in the
// crate (see `SegmentAnchor`'s own doc) — the OPERATIONS that produce a
// `SegmentAnchor`-bearing chunk stay crate-private, reserved for the
// authorized Vue runtime template emitters; see the static call-site guard.
pub use segmented::SegmentAnchor;

#[cfg(test)]
mod edit_semantics_tests;
#[cfg(test)]
mod mapping_product_tests;
#[cfg(test)]
mod tests;
