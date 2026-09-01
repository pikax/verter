//! There is NO test-only public grant constructor, even in a build where
//! `verter_compiler`'s `test-support` feature is active (this crate's
//! dev-dependency enables it): a grant is obtained only by carving a
//! genuine host-issued admission. A reintroduced feature-gated public
//! `mint_for_tests` would compile here and fail the guard.

use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::ProductExecutionGrant;

fn test_mint() -> ProductExecutionGrant {
    ProductExecutionGrant::mint_for_tests(ProductKind::IdeCompanion)
}

fn main() {}
