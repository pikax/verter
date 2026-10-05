// A symbolic call's callee is built only by the frame operation that derives
// its frame root. A literal pairing a type with a chosen root does not
// compile.
use verter_session_query::flow::slice::SymbolicCallee;

fn forge(callee: &SymbolicCallee) -> SymbolicCallee {
    SymbolicCallee {
        ty: callee.ty().clone(),
        frame_root: None,
    }
}

fn main() {}
