use verter_type_engine::resolver_core::fact_validation_port::LiveFactValidation;

fn cannot_escape<P: LiveFactValidation + ?Sized>(port: &P) {
    let clocks = port.aggregate_clock_reader();
    let _workspace = clocks.workspace;
}

fn main() {}
