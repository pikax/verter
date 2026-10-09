use verter_type_engine::resolver_core::fact_validation_port::FactValidation;

fn cannot_escape<P: FactValidation + ?Sized>(port: &P) {
    let checkpoint = port.request_flags().cancellation_checkpoint();
    checkpoint.host();
}

fn main() {}
