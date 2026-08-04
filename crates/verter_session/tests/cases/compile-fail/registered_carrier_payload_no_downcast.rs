use verter_compiler::framework_common::RegisteredCarrierPayload;

fn attempt_downcast(payload: &RegisteredCarrierPayload) {
    let _ = payload.downcast_ref::<()>();
}

fn main() {}
