// The shadow wrapper is built only by the frame operations that computed its
// verdict. Wrapping a carrier with a chosen shadow list does not compile.
use verter_session_query::flow::slice::FrameShadowedExpr;

fn wrap(expr: &FrameShadowedExpr) -> FrameShadowedExpr {
    FrameShadowedExpr {
        inner: Box::new(expr.inner().clone()),
        shadowed: Vec::new().into(),
    }
}

fn main() {}
