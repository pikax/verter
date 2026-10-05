// A gated leaf has no positional constructor taking a type and a chosen
// frame root: the frame derives the root itself.
use verter_session_query::flow::slice::GatedLeaf;

fn forge(leaf: &GatedLeaf) -> GatedLeaf {
    GatedLeaf(leaf.ty().clone(), None)
}

fn main() {}
