use verter_session::VerterHost;

fn read(host: &VerterHost) {
    let _ = host.work_snapshot();
    let _ = host.project_type_store().semantic_graph().work_snapshot();
}

fn main() {}
