use verter_type_engine::cache_runtime::OwnedAdmission;

fn cannot_block(mut handoff: OwnedAdmission<u32, u64>) {
    handoff.wait();
}

fn main() {}
