use std::sync::Arc;

use verter_language::registered_source_authority::{
    RegisteredSourceSnapshot, RegisteredSourceSnapshotId,
};

fn splice(id: RegisteredSourceSnapshotId, bytes: Arc<str>) {
    let _ = RegisteredSourceSnapshot::from_id_and_source(id, bytes);
}

fn main() {}
