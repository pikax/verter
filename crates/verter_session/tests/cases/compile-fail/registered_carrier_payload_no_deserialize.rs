use verter_compiler::framework_common::RegisteredCarrierPayload;

fn attempt_deserialize(bytes: &[u8]) {
    let _: RegisteredCarrierPayload = serde_json::from_slice(bytes).unwrap();
}

fn main() {}
