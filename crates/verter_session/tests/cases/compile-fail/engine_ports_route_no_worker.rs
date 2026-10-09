use verter_type_engine::resolver_core::request_ports::RouteLookup;

fn cannot_escape<P: RouteLookup + ?Sized>(port: &P) {
    if let Some(record) = port.routed_shallow_state("/fixture.ts") {
        record.decl_bodies();
    }
}

fn main() {}
