// The only transformation of a gated leaf is literal widening. An
// arbitrary rewrite of the type under the verdict it already carries does
// not compile.
use verter_session_query::flow::slice::GatedLeaf;

fn rewrite(leaf: GatedLeaf) -> GatedLeaf {
    leaf.map_ty(|ty| ty)
}

fn main() {}
