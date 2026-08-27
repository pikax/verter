//! Compile-fail fixture: the legacy workspace project-resolver surface is absent.
//!
//! Resolution authority lives in `verter_semantic::resolver_core`. If the old
//! workspace-owned type is restored as a wrapper or alias, this fixture compiles
//! and the trybuild guard fails.

use verter_workspace::resolver::ProjectResolver;

fn main() {
    let _: Option<ProjectResolver> = None;
}
