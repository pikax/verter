use std::sync::Arc;

use verter_language::registered_source_authority::{
    CanonicalFileId, RegisteredSourceSnapshot, RegisteredSourceSnapshotId,
};

fn forge(id: RegisteredSourceSnapshotId, canonical: CanonicalFileId) {
    let _ = RegisteredSourceSnapshot {
        id,
        canonical,
        byte_len: 0,
        source: Arc::from("caller bytes"),
    };
}

fn main() {}
