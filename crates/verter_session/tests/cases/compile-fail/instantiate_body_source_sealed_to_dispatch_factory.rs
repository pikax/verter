//! Compile-FAIL fixture: `InstantiateContext`'s source-kind constructors
//! (`file_backed` / `non_file`) are sealed to the dispatch-owned factory —
//! `pub(crate)` visibility plus the dispatch-minted `BodySourceWitness`
//! parameter — so an out-of-factory production construction of a body
//! source kind fails to compile. The ONLY production builder is the
//! `ProjectSemanticDispatch::instantiate_context_for` choke point, which
//! owns the deterministic non-file/file-backed mapping; test fixtures use
//! the `*_for_tests` mints (compiled out of release builds).
//!
//! This fixture lives outside the `verter_session` crate (compiled as a
//! trybuild integration test) and attempts both constructor calls. The
//! compile must FAIL with privacy errors. If either constructor were ever
//! made `pub` (witness-less) again, these lines would COMPILE and trybuild
//! would fail this fixture.

use verter_session::semantic_query::{
    InstantiateContext, ProjectionMode, ProjectionReductionContext,
};

fn main() {
    let prc = ProjectionReductionContext::published(ProjectionMode::Expanded);
    // The compile-fail discrimination points: both source-kind
    // constructors are gated behind `pub(crate)` (and require the
    // dispatch-minted witness). trybuild captures the privacy errors.
    let _non_file = InstantiateContext::non_file(prc, Default::default());
    let _file_backed = InstantiateContext::file_backed;
}
