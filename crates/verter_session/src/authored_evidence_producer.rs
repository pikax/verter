//! Crate-private authored-evidence producer adapters.
//!
//! These adapters keep the unsafe lower-crate mint outside the authority
//! scopes that must remain entirely safe Rust. Each safe entry point accepts
//! an inseparable producer/admission row and derives the locator/text pair
//! from that row where possible.

use std::sync::Arc;

use verter_session_query::analysis::types::{AnalyzedEmitField, AnalyzedPropField};
use verter_type_expr::{AuthoredSourceMint, AuthoredTypeEvidence};

pub(crate) fn from_analyzed_prop(analysis: &AnalyzedPropField) -> Option<AuthoredTypeEvidence> {
    let (locator, text) = analysis
        .payload
        .as_ref()
        .zip(analysis.type_annotation.as_deref())?;
    // SAFETY: both values were borrowed from this one analyzer-produced prop
    // row; callers cannot provide the locator and text independently.
    let mint = unsafe { AuthoredSourceMint::new_unchecked() };
    Some(AuthoredTypeEvidence::from_macro_payload(
        &mint,
        locator,
        Arc::from(text),
    ))
}

pub(crate) fn from_analyzed_emit(analysis: &AnalyzedEmitField) -> Option<AuthoredTypeEvidence> {
    let (locator, text) = analysis
        .payload
        .as_ref()
        .zip(analysis.payload_type.as_deref())?;
    // SAFETY: both values were borrowed from this one analyzer-produced emit
    // row; callers cannot provide the locator and text independently.
    let mint = unsafe { AuthoredSourceMint::new_unchecked() };
    Some(AuthoredTypeEvidence::from_macro_payload(
        &mint,
        locator,
        Arc::from(text),
    ))
}
