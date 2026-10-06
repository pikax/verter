// The symbolic call variant no longer takes a type and a chosen frame root.
use verter_session_query::flow::slice::{SliceCall, SymbolicCallee};

fn forge(callee: &SymbolicCallee) -> SliceCall {
    SliceCall::Symbolic(callee.ty().clone(), None)
}

fn main() {}
