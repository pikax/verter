//! Vue-specific OXC parsing utilities.
//!
//! This module provides parsers for Vue template directives that require
//! special handling beyond standard JavaScript/TypeScript parsing.
//!
//! - `v-for` expressions: `item of items`, `(item, index) in list`
//! - `v-slot` expressions: `{ data }`, `{ item, index = 0 }`

mod script_generic;
mod span;
mod vfor;
mod vslot;

mod script;

pub use script_generic::{
    parse_generic, GenericInfo, GenericParam, GenericParseResult, GENERIC_WRAPPER_PREFIX,
    GENERIC_WRAPPER_SUFFIX,
};
pub use span::{
    adjust_diagnostics_spans, adjust_expression_spans, adjust_formal_parameters_spans,
    adjust_program_spans, subtract_formal_parameters_spans,
};
pub use vfor::{
    extract_vfor_positions, parse_vfor, parse_vfor_with_bindings, VForParseResult, VForWithBindings,
};
pub use vslot::{parse_vslot, parse_vslot_with_bindings, VSlotParseResult, VSlotWithBindings};

pub use script::*;
