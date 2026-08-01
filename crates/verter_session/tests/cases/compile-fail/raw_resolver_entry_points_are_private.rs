//! Compile-fail fixture: raw project-resolver entry points are not public.
//!
//! Engine-owned resolution is the only production boundary allowed to mint a
//! resolution fact signature. If any call below becomes public, the pinned
//! compiler diagnostic changes and the trybuild guard fails.

use verter_workspace::resolver::ProjectResolver;
use verter_workspace::{
    ProjectOwnership, ResolutionContext, ResolveRequest, WorkspaceRead,
};

fn assert_private(
    resolver: &ProjectResolver,
    reader: &dyn WorkspaceRead,
    request: &ResolveRequest,
    owner: &ProjectOwnership,
    context: ResolutionContext,
) {
    let _ = resolver.resolve_with_reader(reader, request);
    let _ = resolver.resolve_for_project_with_reader(reader, owner, "pkg", context);
    let _ = resolver.preferred_specifier(reader, "/src/main.ts", "/src/dep.ts");
}

fn main() {}
