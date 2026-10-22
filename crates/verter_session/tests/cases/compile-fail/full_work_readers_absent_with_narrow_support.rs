use verter_session::VerterHost;

fn read(host: &VerterHost) {
    let _ = host.provenance_snapshot();
    let _ = host.project_type_store().semantic_graph().stats_snapshot();
}

fn main() {}
