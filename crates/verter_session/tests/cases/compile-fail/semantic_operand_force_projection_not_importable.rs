//! Compile-fail: the force-demand and force-projection vocabulary is
//! crate-internal. Its home module is public, but in a build without the
//! engine's `test-support` feature the request-side demand enum, the derived
//! projection enum and the residual-path segment enum are all crate-private
//! (`test-support` re-exports the projection and segment enums `pub` for the
//! host's test suites only), so an external crate can neither import them nor
//! name them through the module path: a force request is built only through
//! the engine's request constructors, and the force entry that consumes one is
//! itself crate-private. (Kept in its own fixture: a failed import suppresses
//! rustc's privacy pass for the whole crate on the pinned toolchain, so
//! co-locating it with the struct-literal seal fixtures would mask their E0451
//! evidence.)

use verter_type_engine::semantic_query::operand::{ForceProjectionSegment, SemanticOperandForceDemand, SemanticOperandForceProjection};

fn main() {
    let _ = SemanticOperandForceProjection::WholeSurface;
    // The REQUEST-side spelling is equally closed: an external crate can
    // neither name a demand nor hand-build a residual-path segment, so a
    // computed index can only ever reach the boundary as a sealed operand.
    let _ = SemanticOperandForceDemand::WholeSurface;
    let _ = ForceProjectionSegment::Member(todo!());
}
