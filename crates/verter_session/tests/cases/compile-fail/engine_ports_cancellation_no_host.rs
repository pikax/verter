use verter_session::for_tests::Cancellation;

fn cannot_escape<P: Cancellation + ?Sized>(port: &P) {
    let checkpoint = port.cancellation_checkpoint();
    checkpoint.host();
}

fn main() {}
