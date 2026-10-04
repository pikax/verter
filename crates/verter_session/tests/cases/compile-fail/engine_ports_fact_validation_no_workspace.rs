use verter_session::for_tests::FactValidation;

fn cannot_escape<P: FactValidation + ?Sized>(port: &P) {
    let clocks = port.aggregate_clock_reader();
    let _workspace = clocks.workspace;
}

fn main() {}
