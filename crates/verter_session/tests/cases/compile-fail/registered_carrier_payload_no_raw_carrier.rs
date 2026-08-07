use verter_compiler::framework_common::RegisteredCarrierPayload;

fn attempt_raw_carrier(payload: &RegisteredCarrierPayload) {
    let _ = payload.carrier;
}

fn main() {}
