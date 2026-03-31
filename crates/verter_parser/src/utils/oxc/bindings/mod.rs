//! Binding extraction utilities for JavaScript/TypeScript expressions.
//!
//! This module provides utilities for extracting identifier bindings from
//! JavaScript/TypeScript expressions parsed by OXC. It handles:
//!
//! - Simple identifiers
//! - Member expressions (only extracting the root object, not properties)
//! - Function expressions (arrow, regular, named)
//! - Destructuring patterns
//! - Variable declarations
//! - TypeScript-specific constructs (type assertions, generics, satisfies, etc.)
//! - Vue-specific expressions (slots, v-for)
//!
//! The extractor properly handles scope, ignoring:
//! - JavaScript keywords (true, false, null, undefined, this, super, etc.)
//! - Function parameters within their scope
//! - Variables declared within block scopes
//!
//! # Example
//!
//! ```ignore
//! use verter_parser::utils::bindings::{extract_bindings_from_expression, BindingContext};
//!
//! let allocator = Allocator::default();
//! let parser = Parser::new(&allocator, "foo + bar", SourceType::tsx());
//! let expr = parser.parse_expression().unwrap();
//! let ctx = BindingContext::new(0);
//! let result = extract_bindings_from_expression(&expr, "foo + bar", ctx);
//!
//! assert_eq!(result.non_ignored_binding_names(), vec!["foo", "bar"]);
//! ```

mod expression;
mod helpers;
pub mod keywords;
mod slot;
mod types;
mod vfor;

// Re-export main types
pub use types::{
    Binding, BindingContext, BindingExtractionResult, Dynamism, FunctionBinding, LiteralBinding,
    ParamBytes, ParameterBindingsResult,
};

// Re-export main functions
pub use expression::{extract_bindings_from_expression, extract_bindings_from_program};
pub use slot::{extract_bindings_from_formal_parameters, extract_slot_bindings};
pub use vfor::extract_vfor_bindings;

// Re-export keyword and global detection for advanced usage
pub use keywords::{is_global, is_keyword};

// Re-export helpers for advanced usage
pub use helpers::{
    collect_assignment_target_locals, collect_assignment_target_locals_array,
    collect_assignment_target_locals_object, collect_assignment_target_maybe_default_locals,
    collect_chain_element_references, collect_expression_references, collect_pattern_locals,
    collect_pattern_references, collect_setup_binding_refs,
    collect_ts_type_references_from_expression, collect_type_references,
};

// Re-export span-based helpers (avoiding self-referential struct issues)
pub use helpers::{
    collect_chain_element_reference_spans, collect_expression_reference_spans,
    collect_pattern_local_spans, collect_pattern_reference_spans,
    collect_ts_type_reference_spans_from_expression, collect_type_reference_spans,
};
