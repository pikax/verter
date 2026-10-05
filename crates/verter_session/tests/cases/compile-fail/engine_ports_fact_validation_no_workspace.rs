use verter_session::for_tests::LiveFactValidation;

fn cannot_escape<P: LiveFactValidation + ?Sized>(port: &P) {
    let clocks = port.aggregate_clock_reader();
    let _workspace = clocks.workspace;
}

fn main() {}
