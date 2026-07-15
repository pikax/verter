//! Compile-FAIL fixture: an R6-forbidden dimension cannot be used as a session
//! query-identity key dimension. `SemanticNodeId` is a generation-local graph
//! ordinal — exactly what R6 forbids in a content-free key. It has no
//! `R6KeyDimension` impl, and the trait is SEALED (its supertrait is private),
//! so no downstream code can give it one. Passing it to `assert_r6_key_dimension`
//! fails the bound — a compile-time proof that a forbidden dimension cannot
//! occupy a `LocatorLoweringKey` env-dimension position.

use verter_session::assert_r6_key_dimension;
use verter_session::semantic_query::SemanticNodeId;

fn main() {
    assert_r6_key_dimension::<SemanticNodeId>();
}
