//! Code transformation utilities for efficient string manipulation with source map support.
//!
//! The [`CodeTransform`] type allows you to make surgical edits to source code while tracking
//! the original positions for source map generation. Unlike naive string building, it only
//! constructs the final output when explicitly requested via [`CodeTransform::to_string`].
//!
//! # Example
//!
//! ```
//! use verter_core::code_transform::{CodeTransform, SourceMapOptions};
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
//! let output = code.to_string();
//! assert_eq!(output, "// Generated\nconst x = 'new';\nconst y = 'test';\nexport { x, y };");
//!
//! // Generate a source map
//! let map = code.generate_map(
//!     SourceMapOptions::new()
//!         .with_source("input.js")
//!         .with_file("output.js")
//! );
//! ```

mod chunk;
#[allow(clippy::module_inception)]
mod code_transform;
mod source_map;

pub use code_transform::CodeTransform;
pub use source_map::SourceMapOptions;

#[cfg(test)]
mod tests;
