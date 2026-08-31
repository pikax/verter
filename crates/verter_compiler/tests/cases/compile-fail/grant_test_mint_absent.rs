//! There is NO test-only public grant constructor: a grant is obtained
//! only by carving a genuine host-issued admission. If a public
//! `mint_for_tests` reappeared un-gated, this line would compile and the
//! guard would fail.

use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::ProductExecutionGrant;

fn test_mint() -> ProductExecutionGrant {
    ProductExecutionGrant::mint_for_tests(ProductKind::IdeCompanion)
}

fn main() {}
