use verter_type_engine::resolver_core::request_ports::ExecutionSubmission;

fn cannot_escape<P: ExecutionSubmission + ?Sized>(port: &P) {
    let binding = port.attach_engine();
    let _graph = binding.graph;
}

fn main() {}
