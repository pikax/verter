use verter_type_engine::resolver_core::request_ports::Cancellation;

fn cannot_escape<P: Cancellation + ?Sized>(port: &P) {
    let checkpoint = port.cancellation_checkpoint();
    checkpoint.host();
}

fn main() {}
