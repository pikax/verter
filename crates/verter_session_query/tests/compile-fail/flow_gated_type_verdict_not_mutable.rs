// A built gated type cannot have its verdict replaced or cleared in place,
// and shadow entries are added only by the signature operations.
use verter_session_query::flow::slice::GatedType;

fn clear(gated: &mut GatedType) {
    gated.shadowed = Vec::new().into();
}

fn widen(gated: &mut GatedType) {
    gated.add_shadowed(Vec::new());
}

fn main() {}
