use verter_session::for_tests::ExecutionSubmission;

fn cannot_escape<P: ExecutionSubmission + ?Sized>(port: &P) {
    let binding = port.attach_engine();
    let _graph = binding.graph;
}

fn main() {}
