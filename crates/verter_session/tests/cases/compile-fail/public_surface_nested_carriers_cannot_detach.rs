//! Compile-fail: a surface borrowed from its owning read cannot be copied
//! out, and neither can its nested arrays, members, entries or index
//! signatures. Each would carry graph operands away from the lease that
//! owns them.

use std::sync::Arc;
use verter_type_engine::semantic_query::{
    IndexSignature, SemanticNodeId, SurfaceEntry, SurfaceMember, SurfaceView,
};

fn detach_surface(view: &SurfaceView) -> SurfaceView {
    view.clone()
}

fn detach_entries(view: &SurfaceView) -> Arc<[SurfaceEntry]> {
    Arc::clone(&view.entries)
}

fn detach_signatures(view: &SurfaceView) -> Arc<[SemanticNodeId]> {
    Arc::clone(&view.call_signatures)
}

fn detach_member(member: &SurfaceMember) -> SurfaceMember {
    member.clone()
}

fn detach_members(view: &SurfaceView) -> Vec<SurfaceMember> {
    view.positive_members().to_vec()
}

fn detach_entry(entry: &SurfaceEntry) -> SurfaceEntry {
    entry.clone()
}

fn detach_index_signature(signature: &IndexSignature) -> IndexSignature {
    signature.clone()
}

fn main() {}
