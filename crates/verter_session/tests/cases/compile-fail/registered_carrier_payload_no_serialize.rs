use verter_compiler::framework_common::RegisteredCarrierPayload;

fn attempt_serialize(payload: &RegisteredCarrierPayload) {
    let _ = serde_json::to_vec(payload);
}

fn main() {}
