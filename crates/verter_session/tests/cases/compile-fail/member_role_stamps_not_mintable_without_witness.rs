//! Compile-FAIL fixture: the witness-gated member role stamps
//! (`MacroOwnBodyStamp` / `MergeRoleStamp`) cannot be minted from raw
//! values and their inner payloads cannot be reached — the inner fields are
//! private, so the tuple constructors are not visible (E0423) and field
//! access fails (E0616). The ONLY producers are the neutral consts and the
//! `ProjectionReductionContext` / analyzed-macro-kind witness methods, which
//! is what makes the role-free locator shape a capability, not a
//! convention. If an inner field were ever made public, these lines would
//! COMPILE and trybuild would fail this fixture.

use verter_session::semantic_query::{MacroOwnBodyStamp, MemberMergeRole, MergeRoleStamp};

fn main() {
    let _ = MacroOwnBodyStamp(true);
    let _ = MergeRoleStamp(MemberMergeRole::OwnBody);
    let neutral = MacroOwnBodyStamp::NEUTRAL;
    let _ = neutral.0;
}
