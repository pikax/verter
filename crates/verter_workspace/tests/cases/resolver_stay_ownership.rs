//! Per-definition crate ownership for the definitions required to STAY in this
//! crate while the module resolver remains semantic-owned.
//!
//! **Compiler assertions, not scans.** Each `use` names a definition at the
//! module path this crate must keep exposing. A definition that is moved,
//! renamed, made private, or removed from the workspace boundary does not compile
//! here, and the canonical gate reports it as a build failure of this crate's
//! test target.
//!
//! Why this is not covered by the dependency-closure test: that test decides
//! whether an EDGE crosses a crate boundary. A STAY definition dragged into the
//! semantic crate along with the resolver creates no upward edge at all — the
//! closure stays legal and the definition is simply in the wrong crate. Only
//! naming it here decides that.
//!
//! `ProjectMembership` is asserted at both ruled public paths. Assignment
//! between them proves the crate-root surface is a re-export of the membership
//! module's nominal type rather than a second vocabulary.

use verter_workspace::fact_read_set::FactReadSet;
use verter_workspace::membership::FallbackMembership;
use verter_workspace::membership::ProjectMembership as MembershipModuleProjectMembership;
use verter_workspace::membership::SupportedExtensions;
use verter_workspace::traits::ResolverSnapshot;
use verter_workspace::traits::WorkspaceRead;

fn assert_sized_and_owned<T>() -> usize {
    core::mem::size_of::<T>()
}

/// Object safety is not the point; naming the trait at its path is. A `dyn`
/// reference forces the trait to exist and stay object-safe, which is the form
/// every consumer of it uses.
fn assert_trait_owned(_: Option<&dyn WorkspaceRead>, _: Option<&dyn ResolverSnapshot>) {}

#[test]
fn stay_class_definitions_remain_owned_by_the_workspace_crate() {
    let _ = assert_sized_and_owned::<SupportedExtensions>();
    let _ = assert_sized_and_owned::<FallbackMembership>();
    let _ = assert_sized_and_owned::<FactReadSet>();
    let root_membership = verter_workspace::ProjectMembership::MatchAll;
    let _: MembershipModuleProjectMembership = root_membership;
    assert_trait_owned(None, None);

    // `CANDIDATE_CAP` is a value, not a type: reading it proves the constant is
    // still exported here, and asserting its value would be asserting policy
    // this file does not own, so only its presence and type are checked.
    let cap: usize = verter_workspace::fact_cache::CANDIDATE_CAP;
    assert!(
        cap > 0,
        "CANDIDATE_CAP must remain a positive workspace-owned bound"
    );
}
