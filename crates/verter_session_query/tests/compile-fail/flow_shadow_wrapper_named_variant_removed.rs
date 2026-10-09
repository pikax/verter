// The shadow wrapper variant no longer takes an inner carrier and a shadow
// list directly.
use verter_session_query::flow::slice::{FrameShadowedExpr, SliceExpr};

fn wrap(expr: &FrameShadowedExpr) -> SliceExpr {
    SliceExpr::FrameShadowed {
        inner: Box::new(expr.inner().clone()),
        shadowed: Vec::new().into(),
    }
}

fn main() {}
