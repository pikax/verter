//! Compile-fail: a whole graph payload, and a whole surface, has no
//! construction copy. Their lists and text are the payload's own, so a copy
//! would allocate fresh backing — or alias the payload's lists — that no
//! owner charges. A builder copies member records, which carry only owned
//! names they share with the payload.

use verter_type_engine::semantic_query::{SemanticNodeData, SurfaceView};
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

fn copy_payload(store: &SemanticGraphStore, payload: &SemanticNodeData) -> Option<SemanticNodeData> {
    store.copy_in_scope(payload)
}

fn copy_surface(store: &SemanticGraphStore, surface: &SurfaceView) -> Option<SurfaceView> {
    store.copy_in_scope(surface)
}

fn main() {}
