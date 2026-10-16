//! Compile-fail: a builder's construction copy is requested only through the
//! store, which roots the copy's operands in an active construction scope;
//! the copy token cannot be named or forged outside it.

use verter_type_engine::semantic_query::SurfaceMember;
use verter_type_engine::semantic_query_memo::construction_copy::ConstructionCopy;

fn copy_outside_store(member: &SurfaceMember) -> SurfaceMember {
    member.copy_with(verter_type_engine::semantic_query_memo::construction_copy::sealed::CopyToken(()))
}

fn main() {}
