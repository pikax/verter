//! Compile-fail: the force-demand vocabulary is crate-internal. Its home
//! module is public, but the request-side demand enum is `pub(crate)`, so an
//! external crate can neither import it nor name it through the module path:
//! a force request is built only through the engine's request constructors,
//! and the force entry that consumes one is itself crate-private. (Kept in
//! its own fixture: a failed import suppresses rustc's privacy pass for the
//! whole crate on the pinned toolchain, so co-locating it with the
//! struct-literal seal fixtures would mask their E0451 evidence.)

use verter_type_engine::semantic_query::operand::SemanticOperandForceDemand;

fn main() {
    // The REQUEST-side spelling is closed: an external crate cannot name a
    // demand, so a residual path reaches the boundary only through a request.
    let _ = SemanticOperandForceDemand::WholeSurface;
}
