use verter_type_engine::resolver_core::fact_validation_port::LiveFactValidation;

fn cannot_escape<P: LiveFactValidation + ?Sized>(port: &P) {
    let clocks = port.request_snapshot().clocks();
    let _workspace = clocks.workspace;
}

fn main() {}
