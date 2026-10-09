//! Reference generator of the flow-transparent callback check: an exact,
//! uncapped, linear-size representation of template condition narrowing for
//! callback bodies, generated from a typed plan.
//!
//! The production IDE emitter integrates this representation directly into
//! its JSX output (see [`super::flow`]): chains and `v-for` frames are
//! immediately invoked blocks, and callbacks are re-narrowed from snapshots of
//! the outer references they read. This module keeps the representation's
//! executable reference — compiled only for tests and the `test-support`
//! feature — together with the contract fixtures both are held to: the
//! reference generator's own fixtures ([`fixtures`]) and the same cases as
//! complete SFCs for the production route ([`ide_fixtures`]).
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
//!   [`generator::GuardStrategy::ReplayPath`] reproduces condition-path
//!   replay, the negative control of every growth check.
//! - [`builder::build_plan`] builds a plan from a real parsed template:
//!   the parser's conditional chains, `OxcParsedAst::scopes` handles, the
//!   shared binding resolver's accessor prefixes, and the production
//!   outer-reference analysis ([`super::flow::outer_refs`]).

mod deps {
    pub(super) use crate::code_transform::CodeTransform;
    pub(super) use oxc_allocator::Allocator;
}

pub mod builder;
pub mod fixtures;
pub mod generator;
pub mod ide_fixtures;
pub mod oracle;
pub mod seam;

#[cfg(test)]
mod tests;
