// A gated type's verdict is computed by the operation that builds it. A
// literal with a chosen shadow list does not compile.
use verter_session_query::flow::slice::GatedType;

fn forge(gated: &GatedType) -> GatedType {
    GatedType {
        ty: gated.ty().clone(),
        shadowed: Vec::new().into(),
    }
}

fn main() {}
