//! A value refusal cannot be minted around its declared-option rule.
//!
//! `CompileRequestError::malformed_option_value` is the one constructor,
//! and it holds the set of options a value refusal is allowed to name —
//! every member of that set has a request field, so the refusal names a
//! path the caller actually wrote. The variant is `#[non_exhaustive]`, so
//! no other crate can write its struct literal and reach the refusal
//! around that rule.

use verter_compiler::compile_request::{CompileRequestError, FrameworkOption, VueOption};

fn forge() -> CompileRequestError {
    CompileRequestError::MalformedOptionValue {
        option: FrameworkOption::Vue(VueOption::ParserOptionsOnWarn),
        value: "anything".to_string(),
    }
}

fn main() {}
