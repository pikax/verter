// A gated leaf is built only by the frame operation that derives its frame
// root from the indexed root occurrence, or by the constructors for an
// answer that names nothing. A literal pairing a type with a chosen frame
// root does not compile.
use verter_session_query::flow::slice::GatedLeaf;

fn forge(leaf: &GatedLeaf) -> GatedLeaf {
    GatedLeaf {
        ty: leaf.ty().clone(),
        frame_root: None,
    }
}

fn main() {}
