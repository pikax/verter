//! Compile-fail: the force-projection vocabulary is crate-internal. Its home
//! module is public, but the enum itself is `pub(crate)`, so an external
//! crate can neither import it nor name it through the module path. (Kept in
//! its own fixture: a failed import suppresses rustc's privacy pass for the
//! whole crate on the pinned toolchain, so co-locating it with the
//! struct-literal seal fixtures would mask their E0451 evidence.)

use verter_session::semantic_query::operand::{
    ForceProjectionSegment, SemanticOperandForceDemand, SemanticOperandForceProjection,
};

fn main() {
    let _ = SemanticOperandForceProjection::WholeSurface;
    // The REQUEST-side spelling is equally closed: an external crate can
    // neither name a demand nor hand-build a residual-path segment, so a
    // computed index can only ever reach the boundary as a sealed operand.
    let _ = SemanticOperandForceDemand::WholeSurface;
    let _ = ForceProjectionSegment::Member(todo!());
}
