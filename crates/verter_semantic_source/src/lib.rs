#![allow(clippy::too_many_arguments)]
//! # verter_semantic_source — source-side declaration and flow lowering
//!
//! Owns the lazy declaration-body memo ([`decl_body_memo`]), the retained
//! parse lowering service ([`decl_lowering`]), the slice-gated flow content
//! lowering ([`flow_slice_content`]), the retained eval-program parse
//! ([`parsed_eval_program`]) and the source helpers they share: the
//! cross-declaration lenses, the `typeof` dependency traversal, the Svelte
//! rune ambient inventory, locator span recovery and the exact content hash.
//!
//! The crate depends on the syntax front-end and the query boundary only. It
//! never depends on the type engine, the session host, the workspace, the
//! compiler, the concrete scheduler, the protocol or any provider.

#[macro_use]
extern crate verter_debug_assert;

pub mod decl_body_memo;
pub mod decl_lowering;
pub mod flow_slice_content;
pub mod locator_span_recovery;
#[cfg(test)]
mod locator_span_recovery_tests;
pub mod parsed_eval_program;
pub mod rune_ambient;
pub mod source_hash;
pub mod source_lens;
pub mod typeof_dependencies;
