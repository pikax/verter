//! Compile-fail fixture: workspace resolution exposes no duplicate project resolver.
//!
//! The workspace uses `verter_resolution::ModuleResolverCore` directly. A
//! workspace-local ProjectResolver wrapper or alias would compile this fixture
//! and violate that boundary.

use verter_workspace::resolver::ProjectResolver;

fn main() {
    let _: Option<ProjectResolver> = None;
}
