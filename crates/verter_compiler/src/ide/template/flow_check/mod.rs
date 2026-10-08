//! Flow-transparent callback checking: an exact, uncapped, linear-size
//! representation of template condition narrowing for callback bodies.
//!
//! The current IDE emitter re-emits the full condition path (every enclosing
//! positive and every predecessor negation) inside each nested chain and each
//! callback, which is quadratic in chain length. This module is the executable
//! replacement representation. It is compiled only for tests and the
//! `test-support` feature: no production path calls it, and production
//! activation belongs to the current-emitter integration.
//!
//! # Seam
//!
//! - Input: [`seam::CheckPlan`] — resolved conditions with authored spans,
//!   ordered branches with explicit predecessor links, lexical scope keys
//!   (one per template lexical scope handle) with parent links, and callbacks
//!   with their authored function, contextual contract and outer references.
//! - Output: [`seam::GeneratedCheck`] — the generated TypeScript, its
//!   authored mappings, deterministic work counters and byte layout.
//! - [`generator::generate`] is the only lowering. It reads the plan and
//!   writes through one `CodeTransform`; it names no parser, scope, resolver
//!   or projection type (its private `deps` module is its entire import surface), which the
//!   compile boundary in `tests/cases/flow_check_boundary.rs` enforces.
//! - [`builder::build_plan`] builds a plan from a real parsed template:
//!   the parser's conditional chains, `OxcParsedAst::scopes` handles, the
//!   shared binding resolver's accessor prefixes and the OXC expression ASTs.
//!   It is how the tests reach the generator from source-backed fixtures.
//!
//! # Production call site
//!
//! The IDE template walk (`ide::template::walk_element`) already resolves
//! each condition, knows each chain's members, each element's scope handles
//! and each callback's contextual type. Integration replaces the per-element
//! guard construction (`ide::condition::generate_condition_text` and its
//! block/ternary guard callers) by collecting a `CheckPlan` during that walk
//! and calling `generate` once per template; the walk's JSX output keeps its
//! role for element and component typing.

mod deps {
    pub(super) use crate::code_transform::CodeTransform;
    pub(super) use oxc_allocator::Allocator;
}

pub mod builder;
pub mod fixtures;
pub mod generator;
pub mod oracle;
pub mod seam;

#[cfg(test)]
mod tests;
