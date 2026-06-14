//! Vue-specific OXC parsing utilities.
//!
//! This module provides parsers for Vue template directives that require
//! special handling beyond standard JavaScript/TypeScript parsing.
//!
//! - `v-for` expressions: `item of items`, `(item, index) in list`
//! - `v-slot` expressions: `{ data }`, `{ item, index = 0 }`

mod script_generic;
mod span;
mod span_shift;
mod vfor;
mod vslot;

mod script;

pub use script_generic::{
    parse_generic, GenericInfo, GenericParam, GenericParseResult, GENERIC_WRAPPER_PREFIX,
    GENERIC_WRAPPER_SUFFIX,
};
pub use span::{
    adjust_diagnostics_spans, adjust_expression_spans, adjust_formal_parameters_spans,
    adjust_program_spans,
};
pub use span_shift::shift_formal_parameters_spans;
pub use vfor::{
    extract_vfor_positions, parse_vfor, parse_vfor_sliced, parse_vfor_with_bindings,
    parse_vfor_with_bindings_sliced, VForParseResult, VForWithBindings,
};
pub use vslot::{
    parse_vslot, parse_vslot_sliced, parse_vslot_with_bindings, parse_vslot_with_bindings_sliced,
    VSlotParseResult, VSlotWithBindings,
};

pub use script::*;
