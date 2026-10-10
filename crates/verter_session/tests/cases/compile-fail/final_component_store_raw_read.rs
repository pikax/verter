use verter_type_engine::component_meta_result_db::{ComponentMetaResultDb, ComponentMetaResultKey};

fn raw_read(db: &ComponentMetaResultDb<u32>, key: &ComponentMetaResultKey) {
    let _ = db.candidate(key, [0; 16]);
}

fn main() {}
