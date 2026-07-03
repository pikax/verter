//! Compile-FAIL fixture: a forbidden R6 dimension NESTED inside a composite key
//! position is still not key-safe. `SemanticNodeId` is a generation-local graph
//! ordinal — R6-forbidden — and has no `R6KeySafe` impl. The `R6KeySafe`
//! container forwarding requires the ELEMENT to be key-safe, so
//! `Option<SemanticNodeId>` (a forbidden dimension nested in a container) fails
//! the `assert_r6_key_safe` bound. This proves a forbidden dimension cannot be
//! laundered into a key by nesting it inside a composite field — the check is
//! not limited to standalone dimensions.

use verter_session::locator_identity::assert_r6_key_safe;
use verter_session::semantic_query::SemanticNodeId;

fn main() {
    assert_r6_key_safe::<Option<SemanticNodeId>>();
}
